mod ime;
use std::{
    collections::{
        hash_map::DefaultHasher,
        BTreeSet,
        HashMap,
        HashSet,
    },
    fs,
    hash::{
        Hash,
        Hasher,
    },
    path::Path,
    time::{
        SystemTime,
        UNIX_EPOCH,
    },
};

use bevy::{
    ecs::system::SystemParam,
    prelude::*,
};
use bevy_egui::{
    egui::{
        self,
        epaint::CircleShape,
        Context,
        Id,
        Memory,
        Modal,
        Modifiers,
        Painter,
        Pos2,
        Rect,
        Response,
        Sense,
        Stroke,
        TextureHandle,
        Ui,
        Vec2,
        Widget,
    },
    EguiContexts,
    EguiGlobalSettings,
    EguiPlugin,
    EguiPrimaryContextPass,
};
use bevy_persistent::{
    Persistent,
    StorageFormat,
};
use ime::*;
use rand::RngExt;
use serde::{
    Deserialize,
    Serialize,
};
use tokio_tungstenite::tungstenite::protocol::Message;

use crate::voxel::{
    VoxelCreativeItem,
    VoxelEditMode,
    VoxelEditorState,
    VoxelLightTool,
    VoxelPlayerStandee,
    VoxelPossessionState,
    VoxelTeleportDestination,
    MAX_VOXEL_BRUSH_RADIUS,
};

const CHAT_WINDOW_SIZE: Vec2 = Vec2::new(360.0, 520.0);
const CHAT_WINDOW_MIN_SIZE: Vec2 = Vec2::new(260.0, 260.0);
const CHAT_WINDOW_MAX_SIZE: Vec2 = Vec2::new(720.0, 720.0);
const GROUP_CHAT_MAX_WIDTH: f32 = 520.0;
const GROUP_CHAT_MAX_HEIGHT: f32 = 720.0;
const GROUP_CHAT_MIN_HEIGHT: f32 = 140.0;
const GROUP_CHAT_SEPARATOR_HEIGHT: f32 = 10.0;
const GROUP_MEMBER_CHAT_SIZE: Vec2 = Vec2::new(320.0, 420.0);
const GROUP_BROADCAST_INPUT_HEIGHT: f32 = 96.0;
const GROUP_BROADCAST_INPUT_ROWS: usize = 3;
const GROUP_MEMBER_WINDOW_SIDE_GAP: f32 = 14.0;
const GROUP_MEMBER_WINDOW_TOP_GAP: f32 = 58.0;
const GROUP_MEMBER_WINDOW_BOTTOM_GAP: f32 = GROUP_BROADCAST_INPUT_HEIGHT + 7.0;
const GROUP_MEMBER_WINDOW_MAX_SIZE: Vec2 = Vec2::new(520.0, 620.0);
const CHAT_AUTO_SCROLL_THRESHOLD: f32 = 48.0;
const CHAT_IMAGE_MAX_SIZE: Vec2 = Vec2::new(220.0, 220.0);
const CHARACTER_WINDOW_DEFAULT_WIDTH: f32 = 360.0;
const CHARACTER_WINDOW_MIN_WIDTH: f32 = 320.0;
const CHARACTER_WINDOW_MAX_WIDTH: f32 = 720.0;
const CHARACTER_FIELD_MAX_WIDTH: f32 = 560.0;
const RANDOM_POOL_BATCH_MAX: u32 = 100;
const NAPCAT_EXPORT_DEFAULT_PATH: &str = ".data/willowblossom/exports/messages_export.json";
const NAPCAT_PC_EXPORT_DEFAULT_PATH: &str =
    ".data/willowblossom/exports/player_characters_export.json";
const NAPCAT_CHAT_LIST_EXPORT_DEFAULT_PATH: &str =
    ".data/willowblossom/exports/chat_list_export.json";
const NAPCAT_UNIT_POOL_EXPORT_DEFAULT_PATH: &str =
    ".data/willowblossom/exports/unit_pool_export.json";
const NAPCAT_MOONBERRY_LEGACY_IMPORT_DEFAULT_PATH: &str =
    ".data/willowblossom/imports/moonberry_legacy.json";
const DEEPSEEK_SUMMARY_EXPORT_DEFAULT_PATH: &str =
    ".data/willowblossom/exports/deepseek_summaries_export.json";
const VOXEL_SCENE_EXPORT_DEFAULT_PATH: &str = ".data/willowblossom/exports/voxel_scene_export.json";
const BATTLE_ROUND_EXPORT_DEFAULT_PATH: &str =
    ".data/willowblossom/exports/battle_rounds_export.json";

fn voxel_material_choices() -> [(u8, &'static str, egui::Color32); 10] {
    [
        (
            1,
            "草地",
            egui::Color32::from_rgb(56, 158, 82),
        ),
        (
            2,
            "泥土",
            egui::Color32::from_rgb(97, 51, 23),
        ),
        (
            3,
            "沙子",
            egui::Color32::from_rgb(219, 184, 97),
        ),
        (
            4,
            "水",
            egui::Color32::from_rgb(20, 97, 209),
        ),
        (
            5,
            "岩浆",
            egui::Color32::from_rgb(255, 41, 4),
        ),
        (
            6,
            "金属舰壳",
            egui::Color32::from_rgb(104, 122, 142),
        ),
        (
            7,
            "装甲板",
            egui::Color32::from_rgb(35, 48, 66),
        ),
        (
            8,
            "发光科技板",
            egui::Color32::from_rgb(40, 190, 235),
        ),
        (
            9,
            "深红舰甲",
            egui::Color32::from_rgb(112, 18, 24),
        ),
        (
            10,
            "发光舱门",
            egui::Color32::from_rgb(205, 92, 24),
        ),
    ]
}

fn paint_voxel_creative_item_icon(
    ui: &Ui,
    rect: egui::Rect,
    item: VoxelCreativeItem,
    background: egui::Color32,
) {
    let painter = ui.painter();
    let scale = rect.width().min(rect.height()) * 0.32;
    let center = rect.center() + egui::vec2(0.0, rect.height() * 0.035);
    let point = |x: f32, y: f32| center + egui::vec2(x * scale, y * scale);
    let luminance = u32::from(background.r()) * 299
        + u32::from(background.g()) * 587
        + u32::from(background.b()) * 114;
    let ink = if luminance > 150_000 {
        egui::Color32::from_gray(28)
    } else {
        egui::Color32::WHITE
    };
    let stroke = Stroke::new(
        (rect.width() * 0.055).clamp(1.5, 3.0),
        ink,
    );
    let thin = Stroke::new((stroke.width * 0.65).max(1.0), ink);
    let line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment(
            [point(a.0, a.1), point(b.0, b.1)],
            stroke,
        );
    };
    let thin_line = |a: (f32, f32), b: (f32, f32)| {
        painter.line_segment([point(a.0, a.1), point(b.0, b.1)], thin);
    };
    let circle = |x: f32, y: f32, radius: f32| {
        painter.circle_stroke(point(x, y), radius * scale, stroke);
    };
    let dot = |x: f32, y: f32, radius: f32| {
        painter.circle_filled(point(x, y), radius * scale, ink);
    };
    let box_outline = |min: (f32, f32), max: (f32, f32)| {
        painter.rect_stroke(
            egui::Rect::from_two_pos(point(min.0, min.1), point(max.0, max.1)),
            1.0,
            stroke,
            egui::StrokeKind::Inside,
        );
    };
    let plus = |x: f32, y: f32, radius: f32| {
        line((x - radius, y), (x + radius, y));
        line((x, y - radius), (x, y + radius));
    };
    let cross = |x: f32, y: f32, radius: f32| {
        line(
            (x - radius, y - radius),
            (x + radius, y + radius),
        );
        line(
            (x + radius, y - radius),
            (x - radius, y + radius),
        );
    };
    let rays = |radius: f32| {
        for (a, b) in [
            ((0.0, -radius), (0.0, -1.0)),
            ((0.0, radius), (0.0, 1.0)),
            ((-radius, 0.0), (-1.0, 0.0)),
            ((radius, 0.0), (1.0, 0.0)),
            (
                (-radius * 0.72, -radius * 0.72),
                (-0.72, -0.72),
            ),
            (
                (radius * 0.72, -radius * 0.72),
                (0.72, -0.72),
            ),
            (
                (-radius * 0.72, radius * 0.72),
                (-0.72, 0.72),
            ),
            (
                (radius * 0.72, radius * 0.72),
                (0.72, 0.72),
            ),
        ] {
            thin_line(a, b);
        }
    };

    match item {
        VoxelCreativeItem::Material(1) => {
            line((-0.9, 0.72), (0.9, 0.72));
            line((-0.55, 0.68), (-0.72, -0.3));
            line((-0.15, 0.68), (0.0, -0.72));
            line((0.3, 0.68), (0.7, -0.42));
        },
        VoxelCreativeItem::Material(2) => {
            for (x, y, radius) in [
                (-0.55, -0.5, 0.16),
                (0.35, -0.58, 0.12),
                (-0.1, 0.0, 0.2),
                (0.6, 0.25, 0.15),
                (-0.5, 0.55, 0.12),
            ] {
                dot(x, y, radius);
            }
        },
        VoxelCreativeItem::Material(3) => {
            thin_line((-0.9, -0.5), (0.25, -0.5));
            thin_line((-0.25, 0.0), (0.9, 0.0));
            thin_line((-0.9, 0.5), (0.45, 0.5));
        },
        VoxelCreativeItem::Material(4) => {
            for y in [-0.42, 0.18, 0.72] {
                thin_line((-0.95, y), (-0.55, y - 0.2));
                thin_line((-0.55, y - 0.2), (-0.1, y));
                thin_line((-0.1, y), (0.35, y + 0.2));
                thin_line((0.35, y + 0.2), (0.9, y));
            }
        },
        VoxelCreativeItem::Material(5) => {
            line((-0.15, -0.95), (-0.45, -0.15));
            line((-0.45, -0.15), (0.05, 0.02));
            line((0.05, 0.02), (-0.2, 0.9));
            line((0.05, 0.02), (0.72, -0.45));
            thin_line((0.05, 0.02), (0.72, 0.58));
        },
        VoxelCreativeItem::Material(6) => {
            box_outline((-0.86, -0.86), (0.86, 0.86));
            for (x, y) in [(-0.58, -0.58), (0.58, -0.58), (-0.58, 0.58), (0.58, 0.58)] {
                dot(x, y, 0.1);
            }
        },
        VoxelCreativeItem::Material(7) => {
            for (a, b) in [
                ((0.0, -0.92), (-0.72, -0.62)),
                ((-0.72, -0.62), (-0.58, 0.42)),
                ((-0.58, 0.42), (0.0, 0.9)),
                ((0.0, 0.9), (0.58, 0.42)),
                ((0.58, 0.42), (0.72, -0.62)),
                ((0.72, -0.62), (0.0, -0.92)),
            ] {
                line(a, b);
            }
        },
        VoxelCreativeItem::Material(8) => {
            box_outline((-0.48, -0.48), (0.48, 0.48));
            rays(0.7);
        },
        VoxelCreativeItem::Material(9) => {
            box_outline((-0.82, -0.82), (0.82, 0.82));
            line((-0.42, 0.82), (0.42, -0.82));
            thin_line((-0.82, 0.25), (0.25, -0.82));
        },
        VoxelCreativeItem::Material(10) => {
            box_outline((-0.72, -0.92), (0.72, 0.92));
            line((0.0, -0.86), (0.0, 0.86));
            dot(-0.28, 0.08, 0.09);
            dot(0.28, 0.08, 0.09);
        },
        VoxelCreativeItem::Material(_) => cross(0.0, 0.0, 0.65),
        VoxelCreativeItem::Light(VoxelLightTool::Point) => {
            circle(0.0, 0.0, 0.4);
            rays(0.62);
        },
        VoxelCreativeItem::Light(VoxelLightTool::DarkPoint) => {
            circle(0.0, 0.0, 0.68);
            painter.circle_filled(
                point(0.3, -0.2),
                0.52 * scale,
                background,
            );
            dot(-0.52, -0.48, 0.08);
        },
        VoxelCreativeItem::Light(VoxelLightTool::Cube) => {
            box_outline((-0.55, -0.55), (0.55, 0.55));
            rays(0.72);
        },
        VoxelCreativeItem::Light(VoxelLightTool::Spot) => {
            box_outline((-0.82, -0.28), (-0.2, 0.28));
            line((-0.2, -0.28), (0.82, -0.72));
            line((-0.2, 0.28), (0.82, 0.72));
            line((-0.82, 0.48), (-0.82, 0.82));
        },
        VoxelCreativeItem::Light(VoxelLightTool::Physics) => {
            circle(0.0, -0.25, 0.45);
            rays(0.62);
            painter.arrow(
                point(0.0, 0.35),
                egui::vec2(0.0, 0.58 * scale),
                stroke,
            );
        },
        VoxelCreativeItem::Light(VoxelLightTool::Edit) => {
            circle(-0.28, -0.28, 0.4);
            rays(0.58);
            line((0.0, 0.82), (0.78, 0.04));
            line((0.55, 0.27), (0.82, 0.54));
        },
        VoxelCreativeItem::Light(VoxelLightTool::Remove) => {
            circle(-0.25, -0.25, 0.42);
            rays(0.58);
            cross(0.45, 0.45, 0.42);
        },
        VoxelCreativeItem::ToolGun => {
            box_outline((-0.82, -0.62), (0.42, 0.18));
            line((0.42, -0.42), (0.92, -0.42));
            line((-0.42, 0.18), (-0.12, 0.92));
            line((-0.12, 0.92), (0.38, 0.92));
            line((0.38, 0.92), (0.18, 0.18));
            box_outline((-0.58, -0.42), (0.12, -0.04));
            dot(-0.22, -0.23, 0.09);
        },
        VoxelCreativeItem::PlayerPossessionTool => {
            circle(0.0, -0.35, 0.28);
            line((-0.58, 0.85), (-0.35, 0.18));
            line((-0.35, 0.18), (0.35, 0.18));
            line((0.35, 0.18), (0.58, 0.85));
            thin_line((-0.92, 0.0), (-0.58, 0.0));
            thin_line((0.58, 0.0), (0.92, 0.0));
            thin_line((0.0, -0.92), (0.0, -0.68));
        },
        VoxelCreativeItem::TeleportTool => {
            circle(0.0, 0.0, 0.76);
            circle(0.0, 0.0, 0.42);
            painter.arrow(
                point(-0.92, 0.0),
                egui::vec2(0.7 * scale, 0.0),
                stroke,
            );
            painter.arrow(
                point(0.22, 0.0),
                egui::vec2(0.7 * scale, 0.0),
                stroke,
            );
        },
        VoxelCreativeItem::Mode(VoxelEditMode::Add) => {
            box_outline((-0.75, -0.75), (0.35, 0.35));
            plus(0.48, 0.48, 0.4);
        },
        VoxelCreativeItem::Mode(VoxelEditMode::Remove) => {
            box_outline((-0.75, -0.75), (0.35, 0.35));
            cross(0.48, 0.48, 0.4);
        },
        VoxelCreativeItem::Mode(VoxelEditMode::Paint) => {
            line((-0.72, 0.72), (0.42, -0.42));
            line((-0.42, 0.9), (0.58, -0.1));
            line((0.35, -0.5), (0.78, -0.82));
            thin_line((-0.82, 0.88), (-0.42, 0.9));
        },
        VoxelCreativeItem::Mode(VoxelEditMode::Physics) => {
            box_outline((-0.58, -0.78), (0.58, 0.35));
            painter.arrow(
                point(0.0, 0.38),
                egui::vec2(0.0, 0.55 * scale),
                stroke,
            );
        },
        VoxelCreativeItem::Mode(VoxelEditMode::Drag) => {
            box_outline((-0.58, -0.58), (0.58, 0.58));
            painter.arrow(
                point(0.0, 0.0),
                egui::vec2(0.72 * scale, 0.0),
                stroke,
            );
        },
        VoxelCreativeItem::Mode(VoxelEditMode::Push) => {
            box_outline((-0.85, -0.48), (-0.15, 0.48));
            painter.arrow(
                point(-0.02, 0.0),
                egui::vec2(0.9 * scale, 0.0),
                stroke,
            );
        },
        VoxelCreativeItem::Mode(VoxelEditMode::Pull) => {
            box_outline((0.15, -0.48), (0.85, 0.48));
            painter.arrow(
                point(0.02, 0.0),
                egui::vec2(-0.9 * scale, 0.0),
                stroke,
            );
        },
        VoxelCreativeItem::Mode(VoxelEditMode::Explode) => {
            dot(0.0, 0.0, 0.18);
            rays(0.3);
        },
    }
}

fn voxel_material_slot(
    ui: &mut Ui,
    material: u8,
    name: &str,
    color: egui::Color32,
    selected_material: u8,
    size: f32,
    shortcut: Option<&str>,
) -> bool {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::click());
    ui.painter().rect_filled(rect, 3, color);
    let stroke = if selected_material == material {
        Stroke::new(3.0, egui::Color32::WHITE)
    } else {
        Stroke::new(1.0, egui::Color32::from_gray(90))
    };
    ui.painter().rect_stroke(
        rect,
        3,
        stroke,
        egui::StrokeKind::Inside,
    );
    paint_voxel_creative_item_icon(
        ui,
        rect,
        VoxelCreativeItem::Material(material),
        color,
    );
    if let Some(shortcut) = shortcut {
        ui.painter().text(
            rect.left_top() + egui::vec2(3.0, 2.0),
            egui::Align2::LEFT_TOP,
            shortcut,
            egui::FontId::proportional(10.0),
            egui::Color32::WHITE,
        );
    }
    let clicked = response.clicked();
    response.on_hover_text(format!("{name} · 创造模式无限使用"));
    clicked
}

fn voxel_creative_item_visual(item: VoxelCreativeItem) -> (&'static str, egui::Color32) {
    match item {
        VoxelCreativeItem::Material(material) => voxel_material_choices()
            .into_iter()
            .find(|(candidate, ..)| *candidate == material)
            .map(|(_, name, color)| (name, color))
            .unwrap_or(("未知方块", egui::Color32::DARK_GRAY)),
        VoxelCreativeItem::Light(tool) => match tool {
            VoxelLightTool::Point => (
                "点光源",
                egui::Color32::from_rgb(255, 205, 112),
            ),
            VoxelLightTool::DarkPoint => (
                "暗色点光",
                egui::Color32::from_rgb(74, 31, 112),
            ),
            VoxelLightTool::Cube => (
                "方块灯",
                egui::Color32::from_rgb(62, 218, 255),
            ),
            VoxelLightTool::Spot => (
                "聚光灯",
                egui::Color32::from_rgb(255, 241, 192),
            ),
            VoxelLightTool::Physics => (
                "物理灯",
                egui::Color32::from_rgb(72, 147, 255),
            ),
            VoxelLightTool::Edit => (
                "灯光编辑器",
                egui::Color32::from_rgb(212, 110, 255),
            ),
            VoxelLightTool::Remove => (
                "移除灯光",
                egui::Color32::from_rgb(150, 42, 42),
            ),
        },
        VoxelCreativeItem::ToolGun => (
            "工具枪",
            egui::Color32::from_rgb(238, 116, 30),
        ),
        VoxelCreativeItem::PlayerPossessionTool => (
            "PL接管器",
            egui::Color32::from_rgb(116, 82, 238),
        ),
        VoxelCreativeItem::TeleportTool => (
            "传送器",
            egui::Color32::from_rgb(50, 184, 210),
        ),
        VoxelCreativeItem::Mode(mode) => match mode {
            VoxelEditMode::Add => (
                "添加工具",
                egui::Color32::from_rgb(62, 162, 88),
            ),
            VoxelEditMode::Remove => (
                "删除工具",
                egui::Color32::from_rgb(164, 54, 54),
            ),
            VoxelEditMode::Paint => (
                "涂色工具",
                egui::Color32::from_rgb(206, 144, 52),
            ),
            VoxelEditMode::Physics => (
                "物理化工具",
                egui::Color32::from_rgb(95, 112, 138),
            ),
            VoxelEditMode::Drag => (
                "拖拽工具",
                egui::Color32::from_rgb(54, 190, 174),
            ),
            VoxelEditMode::Push => (
                "推开工具",
                egui::Color32::from_rgb(61, 132, 196),
            ),
            VoxelEditMode::Pull => (
                "拉近工具",
                egui::Color32::from_rgb(102, 88, 190),
            ),
            VoxelEditMode::Explode => (
                "爆炸工具",
                egui::Color32::from_rgb(225, 83, 28),
            ),
        },
    }
}

#[derive(Clone, Copy)]
enum VoxelCreativeDragPayload {
    Catalog(VoxelCreativeItem),
    Hotbar { slot: usize },
}

fn voxel_creative_item_slot(
    ui: &mut Ui,
    item: Option<VoxelCreativeItem>,
    selected: bool,
    size: f32,
    shortcut: Option<&str>,
) -> egui::Response {
    let (rect, response) = ui.allocate_exact_size(egui::vec2(size, size), Sense::click());
    let (name, color) = item.map(voxel_creative_item_visual).unwrap_or((
        "空快捷栏格",
        egui::Color32::from_gray(35),
    ));
    ui.painter().rect_filled(rect, 3, color);
    ui.painter().rect_stroke(
        rect,
        3,
        Stroke::new(
            if selected { 3.0 } else { 1.0 },
            if selected { egui::Color32::WHITE } else { egui::Color32::from_gray(90) },
        ),
        egui::StrokeKind::Inside,
    );
    if let Some(item) = item {
        paint_voxel_creative_item_icon(ui, rect, item, color);
    }
    if let Some(shortcut) = shortcut {
        ui.painter().text(
            rect.left_top() + egui::vec2(3.0, 2.0),
            egui::Align2::LEFT_TOP,
            shortcut,
            egui::FontId::proportional(10.0),
            egui::Color32::WHITE,
        );
    }
    response.on_hover_text(name)
}

fn voxel_creative_drag_source(
    ui: &mut Ui,
    id: egui::Id,
    payload: VoxelCreativeDragPayload,
    item: VoxelCreativeItem,
    selected: bool,
    size: f32,
    shortcut: Option<&str>,
) -> egui::Response {
    ui.dnd_drag_source(id, payload, |ui| {
        voxel_creative_item_slot(ui, Some(item), selected, size, shortcut)
    })
    .inner
}
const MOONBERRY_SKILL_TYPES: &[&str] = &[
    "法术",
    "道具",
    "异能",
    "动作",
    "血统",
    "职业",
    "召唤物",
    "远程",
];
const MOONBERRY_TARGET_CLASSES: &[&str] = &["无目标", "单目标", "多目标", "范围"];

use crate::{
    battle_round::{
        BattleRoundStore,
        BattleRoundUiState,
        BATTLE_ROUND_EXPORT_VERSION,
    },
    deepseek::{
        DeepseekIOSender,
        DeepseekManager,
        DeepseekPlugin,
        DeepseekRequest,
        DeepseekSummaryBlock,
        DEEPSEEK_SUMMARY_EXPORT_VERSION,
    },
    napcat::{
        character_chaos_output_variance,
        character_damage_attribute_multiplier,
        character_damage_dealt_talent_buffs,
        character_damage_taken_attribute_multiplier,
        character_dying_target_healing_modifier,
        character_effective_skill_mp_cost,
        character_fatigue_walker_available,
        character_healing_attribute_multiplier,
        character_large_hit_damage_taken_modifier,
        character_low_hp_damage_multiplier,
        character_minimum_damage_floor,
        character_minimum_range_meters,
        character_mutual_aid_healing_rate,
        character_next_level_exp,
        character_physical_damage_followup_rate,
        character_physical_damage_lifesteal,
        character_spell_range_multiplier,
        character_wounded_healing_dealt_modifier,
        dying_target_healing_multiplier,
        grant_character_experience,
        is_scene_capture_command_text,
        large_hit_damage_taken_multiplier,
        moonberry_chaos_output_multiplier,
        moonberry_effective_skill_range_radius_with_multiplier,
        moonberry_physical_damage_followup_buff,
        moonberry_skill_type_is_spell,
        normalized_random_pool_counts,
        record_character_damage_taken,
        record_character_healing_taken,
        reset_character_turn_totals,
        skill_rule_args,
        update_character_from_status,
        update_character_from_status_with_config,
        upsert_character_active_buff,
        wounded_healing_dealt_multiplier,
        CampaignMessage,
        CharacterBuffBaseStats,
        CharacterCreationStep,
        CharacterHotbarSlot,
        CharacterInventory,
        CharacterSkillMetadata,
        CharacterSkillSourceKind,
        CharacterStatus,
        ChatGroup,
        ChatTargetExportKind,
        EquipmentSlot,
        ImageData,
        InventoryItem,
        InventoryQuality,
        NapcatIOSender,
        NapcatMessage,
        NapcatMessageChain,
        NapcatMessageChainType,
        NapcatMessageData,
        NapcatMessageManager,
        NapcatMessageType,
        NapcatSendManager,
        NapcatSender,
        PlayerCharacter,
        RandomPool,
        RandomPoolCheckedResult,
        RandomPoolEntry,
        RandomPoolTextResult,
        SkillPoolEntry,
        SkillRuleArgs,
        TextData,
        TrpgBasicConfig,
        TrpgDamageBonusKind,
        TrpgDamageTakenKind,
        TrpgGroup,
        TrpgLegacyNegativeTimer,
        TrpgLegacySendPane,
        TrpgLegacyTeamChatMessage,
        UnitPoolEntry,
        Visibility,
        LEGACY_NEGATIVE_TIMEOUT_MS,
        NAPCAT_MANAGER_EXPORT_VERSION,
    },
    rule_engine::{
        apply_skill_type_damage_default,
        legacy_moonberry_buff_machine_passive_buffs,
        legacy_moonberry_buff_machine_skill_cast_rule_with_context,
        parse_rule_with_named_args,
        Action,
        ActorRef,
        BuffEffect,
        BuffField,
        BuffKind,
        BuffSpec,
        BuffTickAction,
        BuffValue,
        Character as RuleCharacter,
        DamageType,
        LegacyMoonberryPoolArg,
        LegacyMoonberryPoolEntry,
        RuleAst,
        RuleEngineState,
        StatusBlock,
        StatusKey,
        TargetSelector,
        ValueExpr,
    },
    scene::{
        has_legacy_area_marker,
        has_unit_template_standee,
        has_unit_template_token,
        legacy_area_marker_id,
        place_legacy_area_marker,
        place_legacy_area_unit_token,
        place_legacy_world_unit_token,
        place_unit_template_standee,
        place_unit_template_token,
        prune_legacy_area_unit_tokens,
        prune_legacy_world_unit_tokens,
        remove_legacy_area_marker,
        remove_legacy_area_unit_tokens,
        remove_legacy_world_unit_tokens,
        remove_unit_template_standee,
        remove_unit_template_token,
        stamp_legacy_area_marker_voxel_fill,
        stamp_legacy_area_marker_voxel_outline,
        SceneCharacterPositions,
        ScenePlayerCameraPositions,
        ScenePlayerViewRequest,
        VoxelMapRuntimeState,
        VoxelSceneStore,
        VOXEL_SCENE_EXPORT_VERSION,
    },
};
pub struct UIPlugin;
#[derive(Resource)]
pub struct GIFImages {
    images: HashMap<String, Vec<(TextureHandle, u32)>>,
}

#[derive(Default)]
pub(crate) struct ChatScrollState {
    message_count: usize,
    near_bottom: bool,
}

#[derive(Default)]
pub(crate) struct TrpgGroupSettingsState {
    open: bool,
    pool_window_open: bool,
    pool_window_tab: PoolWindowTab,
    new_group_name: String,
    new_random_pool_name: String,
    random_pool_award_target: String,
    random_pool_broadcast_scope: String,
    random_pool_batch_count: u32,
    random_pool_send_status: String,
    random_pool_group_filter: String,
    random_pool_tag_filter: String,
    new_unit_id: String,
    unit_pool_source_target: String,
    focused_group_name: Option<String>,
    pending_character_delete: Option<String>,
    pending_party_delete: Option<(String, String)>,
    pending_turn_zero_reset: Option<String>,
    pending_initial_stats_restore: Option<String>,
    group_reset_status: HashMap<String, String>,
    legacy_send_pane_status: HashMap<String, String>,
    legacy_team_chat_status: HashMap<String, String>,
    legacy_team_chat_edit_drafts: HashMap<String, String>,
    legacy_negative_status: HashMap<String, String>,
    legacy_area_marker_status: HashMap<String, String>,
    unit_pool_scene_status: HashMap<String, String>,
    open_legacy_send_pane_windows: HashSet<(String, String)>,
    open_legacy_team_chat_windows: HashSet<(String, String)>,
    random_pool_entry_drafts: HashMap<String, RandomPoolEntry>,
    unit_pool_draft: UnitPoolEntry,
    skill_pool_draft: SkillPoolEntry,
    item_pool_draft: InventoryItem,
    item_pool_award_target: String,
    party_name_drafts: HashMap<String, String>,
    party_merge_targets: HashMap<(String, String), String>,
    export_path: String,
    pc_export_path: String,
    chat_list_export_path: String,
    unit_pool_export_path: String,
    moonberry_legacy_import_path: String,
    deepseek_summary_export_path: String,
    voxel_scene_export_path: String,
    battle_round_export_path: String,
    import_path: String,
    import_export_status: String,
}

#[derive(Clone, Copy, Default, PartialEq, Eq)]
enum PoolWindowTab {
    #[default]
    Player,
    GroupChat,
    Random,
    Unit,
    Skill,
    Item,
}

#[derive(Default)]
pub(crate) struct CharacterEditState {
    unlocked_status_targets: HashSet<String>,
    gm_status_drafts: HashMap<String, CharacterStatus>,
    buff_drafts: HashMap<String, BuffDraft>,
    pending_character_reset: Option<String>,
    quick_cast_skill_index: HashMap<String, usize>,
    pending_force_cast: Option<(String, usize)>,
    skill_pool_selected_index: HashMap<String, usize>,
    item_pool_selected_index: HashMap<String, usize>,
    exp_award_drafts: HashMap<String, i32>,
}

#[derive(Clone)]
pub(crate) struct BuffDraft {
    name: String,
    kind: BuffKind,
    priority: i32,
    turns_remaining: i32,
    beneficial: bool,
    field: BuffField,
    value: BuffValue,
}

impl Default for BuffDraft {
    fn default() -> Self {
        Self {
            name: "新buff".to_owned(),
            kind: BuffKind::Magic,
            priority: 0,
            turns_remaining: 1,
            beneficial: true,
            field: BuffField::DamageTakenModifier,
            value: BuffValue::Set(0.5),
        }
    }
}

#[derive(SystemParam)]
pub struct UiSystemLocals<'w, 's> {
    has_run_once: Local<'s, bool>,
    new_chat_group_modal_string_open: Local<'s, (String, bool)>,
    chat_input_msgs: Local<'s, HashMap<String, String>>,
    chat_scroll_states: Local<'s, HashMap<String, ChatScrollState>>,
    previous_group_rects: Local<'s, HashMap<String, Rect>>,
    chat_list_edit_target: Local<'s, Option<String>>,
    chat_list_edit_name: Local<'s, String>,
    trpg_group_settings: Local<'s, TrpgGroupSettingsState>,
    character_edit_state: Local<'s, CharacterEditState>,
    quick_character_targets: Local<'s, HashSet<String>>,
    chat_image_textures: Local<'s, HashMap<String, TextureHandle>>,
    chat_turn_count_drafts: Local<'s, HashMap<(String, String), u32>>,
    group_broadcast_scopes: Local<'s, HashMap<String, String>>,
    chat_player_visible_previews: Local<'s, HashMap<String, String>>,
    chat_list_player_visible_filter: Local<'s, Option<String>>,
    voxel_editor: ResMut<'w, VoxelEditorState>,
    voxel_possession: ResMut<'w, VoxelPossessionState>,
    battle_store: Option<ResMut<'w, Persistent<BattleRoundStore>>>,
    player_standees: Query<
        'w,
        's,
        (&'static VoxelPlayerStandee, &'static bevy::prelude::Visibility),
    >,
}

pub struct CircleImageButton {
    image: egui::TextureId,
    size: f32,
}

#[derive(Resource, Serialize, Deserialize)]
pub struct CachedMemory {
    ui_memory: Memory,
}

impl CircleImageButton {
    pub fn new(image: egui::TextureId, size: f32) -> Self { Self { image, size } }
}

fn file_menu_button(
    ui: &mut Ui,
    new_chat_group_modal_open: &mut bool,
    trpg_group_settings_open: &mut bool,
) {
    let new_chat_group_shortcup = egui::KeyboardShortcut::new(Modifiers::COMMAND, egui::Key::G);

    // NOTE: we must check the shortcuts OUTSIDE of the actual "File" menu,
    // or else they would only be checked if the "File" menu was actually open!

    if ui.input_mut(|i| i.consume_shortcut(&new_chat_group_shortcup)) {
        *new_chat_group_modal_open = true
    }

    ui.menu_button("编辑", |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

        if ui
            .add(
                egui::Button::new("新建讨论组")
                    .shortcut_text(ui.ctx().format_shortcut(&new_chat_group_shortcup)),
            )
            .clicked()
        {
            *new_chat_group_modal_open = true
        }

        if ui.button("TRPG设置").clicked() {
            *trpg_group_settings_open = true;
            ui.close();
        }
    });
}

fn tools_menu_button(
    ui: &mut Ui,
    rule_engine_state: &mut RuleEngineState,
    battle_round_state: &mut BattleRoundUiState,
) {
    ui.menu_button("工具", |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

        if ui.button("战斗轮").clicked() {
            battle_round_state.open_panel();
            ui.close();
        }
        if ui.button("规则引擎").clicked() {
            rule_engine_state.open_panel();
            ui.close();
        }
    });
}

fn pool_menu_button(
    ui: &mut Ui,
    manager: &NapcatMessageManager,
    state: &mut TrpgGroupSettingsState,
) {
    let player_targets = sorted_pool_targets(manager, false);
    let group_chat_targets = sorted_pool_targets(manager, true);

    ui.menu_button("池", |ui| {
        ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);

        for (label, tab) in [
            (
                format!("玩家池 ({})", player_targets.len()),
                PoolWindowTab::Player,
            ),
            (
                format!("群聊池 ({})", group_chat_targets.len()),
                PoolWindowTab::GroupChat,
            ),
            (
                format!(
                    "随机池 ({})",
                    manager.random_pools.len()
                ),
                PoolWindowTab::Random,
            ),
            (
                format!("单位池 ({})", manager.unit_pool.len()),
                PoolWindowTab::Unit,
            ),
            (
                format!("技能池 ({})", manager.skill_pool.len()),
                PoolWindowTab::Skill,
            ),
            (
                format!("物品池 ({})", manager.item_pool.len()),
                PoolWindowTab::Item,
            ),
        ] {
            if ui.button(label).clicked() {
                state.pool_window_tab = tab;
                state.pool_window_open = true;
                ui.close();
            }
        }
    });
}

fn pool_management_window(
    ctx: &Context,
    manager: &mut Persistent<NapcatMessageManager>,
    state: &mut TrpgGroupSettingsState,
    napcat_sender: Option<&NapcatIOSender>,
    ime: &mut ImeManager,
    mut scene_store: Option<&mut Persistent<VoxelSceneStore>>,
) {
    if !state.pool_window_open {
        return;
    }

    let player_targets = sorted_pool_targets(manager, false);
    let group_chat_targets = sorted_pool_targets(manager, true);
    let mut open = state.pool_window_open;
    let mut changed = false;
    egui::Window::new("池管理")
        .id(Id::new("pool_management_window"))
        .open(&mut open)
        .default_size(Vec2::new(720.0, 600.0))
        .min_size(Vec2::new(460.0, 320.0))
        .show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.selectable_value(
                    &mut state.pool_window_tab,
                    PoolWindowTab::Player,
                    format!("玩家 ({})", player_targets.len()),
                );
                ui.selectable_value(
                    &mut state.pool_window_tab,
                    PoolWindowTab::GroupChat,
                    format!("群聊 ({})", group_chat_targets.len()),
                );
                ui.selectable_value(
                    &mut state.pool_window_tab,
                    PoolWindowTab::Random,
                    format!(
                        "随机池 ({})",
                        manager.random_pools.len()
                    ),
                );
                ui.selectable_value(
                    &mut state.pool_window_tab,
                    PoolWindowTab::Unit,
                    format!("单位池 ({})", manager.unit_pool.len()),
                );
                ui.selectable_value(
                    &mut state.pool_window_tab,
                    PoolWindowTab::Skill,
                    format!("技能池 ({})", manager.skill_pool.len()),
                );
                ui.selectable_value(
                    &mut state.pool_window_tab,
                    PoolWindowTab::Item,
                    format!("物品池 ({})", manager.item_pool.len()),
                );
            });
            ui.separator();
            egui::ScrollArea::vertical()
                .id_salt("pool_management_scroll")
                .show(ui, |ui| match state.pool_window_tab {
                    PoolWindowTab::Player => pool_target_list_ui(
                        ui,
                        manager,
                        &player_targets,
                        "还没有玩家私聊。",
                    ),
                    PoolWindowTab::GroupChat => pool_target_list_ui(
                        ui,
                        manager,
                        &group_chat_targets,
                        "还没有QQ群聊。",
                    ),
                    PoolWindowTab::Random => {
                        changed |= random_pool_settings_ui(
                            ui,
                            manager,
                            state,
                            &player_targets,
                            napcat_sender,
                            Some(&mut *ime),
                        )
                    },
                    PoolWindowTab::Unit => {
                        changed |= unit_pool_settings_ui(
                            ui,
                            manager,
                            state,
                            &player_targets,
                            scene_store.as_deref_mut(),
                        )
                    },
                    PoolWindowTab::Skill => changed |= skill_pool_settings_ui(ui, manager, state),
                    PoolWindowTab::Item => {
                        changed |= item_pool_settings_ui(ui, manager, state, &player_targets)
                    },
                });
        });
    state.pool_window_open = open;
    if changed {
        manager.persist().ok();
    }
}

fn pool_target_list_ui(
    ui: &mut Ui,
    manager: &NapcatMessageManager,
    targets: &[String],
    empty_text: &str,
) {
    if targets.is_empty() {
        ui.label(empty_text);
        return;
    }
    for target_id in targets {
        ui.horizontal(|ui| {
            ui.label(target_display_name(manager, target_id));
            ui.small(target_id);
        });
    }
}

impl Widget for CircleImageButton {
    fn ui(self, ui: &mut egui::Ui) -> Response {
        let (rect, response) = ui.allocate_exact_size(Vec2::splat(self.size), Sense::click());
        let painter = Painter::new(ui.ctx().clone(), ui.layer_id(), rect);
        painter.add(egui::Shape::Circle(CircleShape {
            center: rect.center(),
            radius: self.size / 2.0,
            fill: Default::default(),
            stroke: Stroke::NONE,
        }));
        painter.add(egui::Shape::image(
            self.image,
            bevy_egui::egui::Rect::from_center_size(rect.center(), Vec2::splat(self.size)),
            bevy_egui::egui::Rect::from_center_size(rect.center(), Vec2::splat(self.size)),
            egui::Color32::WHITE,
        ));
        response
    }
}

impl Plugin for UIPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins(EguiPlugin::default())
            .insert_resource(EguiGlobalSettings {
                auto_create_primary_context: false,
                ..default()
            })
            .add_plugins(ImePlugin)
            .add_plugins(DeepseekPlugin)
            .add_systems(Startup, setup_system)
            .add_systems(
                EguiPrimaryContextPass,
                load_ui_memory.run_if(resource_added::<Persistent<CachedMemory>>),
            )
            .add_systems(
                EguiPrimaryContextPass,
                configure_ui_fonts.after(load_ui_memory),
            )
            .add_systems(
                EguiPrimaryContextPass,
                ui_system
                    .run_if(resource_exists::<Persistent<CachedMemory>>)
                    .run_if(crate::replay::replay_video_capture_inactive)
                    .after(load_ui_memory),
            );
    }
}

pub fn setup_system(mut command: Commands) {
    let config_dir = Path::new(".data").join("willowblossom");
    let cached_memory = Persistent::<CachedMemory>::builder()
        .name("ui_memory")
        .format(StorageFormat::Ron)
        .path(config_dir.join("ui_memory.ron"))
        .default(CachedMemory {
            ui_memory: Memory::default(),
        })
        .revertible(true)
        .revert_to_default_on_deserialization_errors(true)
        .build()
        .expect("failed to init ui memory");
    command.insert_resource(cached_memory);
    command.insert_resource(GIFImages {
        images: HashMap::default(),
    });
}

pub fn configure_ui_fonts(mut egui_context: EguiContexts, mut fonts_configured: Local<bool>) {
    if *fonts_configured {
        return;
    }

    let Ok(ctx) = egui_context.ctx_mut() else {
        return;
    };

    let mut fonts = egui::FontDefinitions::default();
    fonts.font_data.insert(
        "cjk".to_owned(),
        egui::FontData::from_static(include_bytes!(
            "../../assets/fonts/AlibabaHealthFont.ttf"
        ))
        .into(),
    );
    fonts
        .families
        .entry(egui::FontFamily::Proportional)
        .or_default()
        .insert(0, "cjk".to_owned());
    fonts
        .families
        .entry(egui::FontFamily::Monospace)
        .or_default()
        .insert(0, "cjk".to_owned());

    let theme = ctx.theme();
    let mut style = (*ctx.style_of(theme)).clone();
    style.text_styles = [
        (
            egui::TextStyle::Heading,
            egui::FontId::new(18.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Body,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Monospace,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Button,
            egui::FontId::new(14.0, egui::FontFamily::Proportional),
        ),
        (
            egui::TextStyle::Small,
            egui::FontId::new(10.0, egui::FontFamily::Proportional),
        ),
    ]
    .into();

    ctx.set_fonts(fonts);
    ctx.set_style_of(theme, style);
    egui_extras::install_image_loaders(ctx);
    *fonts_configured = true;
}

pub fn load_ui_memory(
    mut egui_context: EguiContexts,
    cached_memory: Res<Persistent<CachedMemory>>,
) {
    let Ok(ctx) = egui_context.ctx_mut() else {
        return;
    };
    let mut memory = cached_memory.ui_memory.clone();
    memory.reset_areas();
    ctx.memory_mut(|m| *m = memory);
}

fn chat_window(
    nickname: &str,
    id: Id,
    rect: Rect,
    ctx: &Context,
    _lens: Vec<usize>,
    messages: &Vec<NapcatMessage>,
    napcat_sender: Option<&NapcatIOSender>,
    target_id: &str,
    chat_input_msgs: &mut Local<HashMap<String, String>>,
    targets: Vec<NapcatSendTarget>,
    ime: &mut ResMut<ImeManager>,
    chat_scroll_states: &mut Local<HashMap<String, ChatScrollState>>,
    group_rects: &HashMap<String, Rect>,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    current_group: Option<&str>,
    group_delta: Option<Vec2>,
    unread_count: usize,
    quick_character_targets: &mut Local<HashSet<String>>,
    image_textures: &mut Local<HashMap<String, TextureHandle>>,
    focused_trpg_group_name: Option<&str>,
    turn_count_drafts: &mut Local<HashMap<(String, String), u32>>,
    chat_player_visible_previews: &mut Local<HashMap<String, String>>,
    rule_engine_state: &mut RuleEngineState,
    mut player_view_request: Option<&mut ScenePlayerViewRequest>,
) {
    let mut window_open = true;
    let mut leave_group = false;
    let constraint_rect =
        if current_group.is_some() { group_member_constraint_rect(rect) } else { rect };
    let window_min_size = egui::vec2(
        CHAT_WINDOW_MIN_SIZE.x.min(constraint_rect.width().max(1.0)),
        CHAT_WINDOW_MIN_SIZE
            .y
            .min(constraint_rect.height().max(1.0)),
    );
    let max_window_size = if current_group.is_some() {
        egui::vec2(
            GROUP_MEMBER_WINDOW_MAX_SIZE
                .x
                .min(constraint_rect.width())
                .max(window_min_size.x),
            GROUP_MEMBER_WINDOW_MAX_SIZE
                .y
                .min(constraint_rect.height())
                .max(window_min_size.y),
        )
    } else {
        egui::vec2(
            CHAT_WINDOW_MAX_SIZE
                .x
                .min(constraint_rect.width())
                .max(window_min_size.x),
            CHAT_WINDOW_MAX_SIZE
                .y
                .min(constraint_rect.height())
                .max(window_min_size.y),
        )
    };
    let window_id = current_group
        .map(|group_name| {
            Id::new((
                group_name,
                target_id,
                "group_member_chat_window_v2",
            ))
        })
        .unwrap_or_else(|| standalone_chat_window_id(id, target_id));
    let mut window = egui::Window::new(nickname)
        .open(&mut window_open)
        .id(window_id)
        .constrain_to(constraint_rect)
        .default_size(CHAT_WINDOW_SIZE)
        .min_size(window_min_size)
        .max_size(max_window_size)
        .max_height(GROUP_CHAT_MAX_HEIGHT);
    if current_group.is_some() {
        if let Some(delta) = group_delta {
            if let Some(member_rect) = ctx.memory(|memory| memory.area_rect(window_id)) {
                window = window.current_pos(member_rect.min + delta);
            }
        }
        window = window.default_pos(group_member_default_pos(
            constraint_rect,
            target_id,
        ));
    }
    let show_character_button = !is_group_chat_target(manager, target_id);
    let trpg_membership_group = focused_trpg_group_name
        .filter(|group_name| manager.trpg_groups.contains_key(*group_name))
        .map(str::to_owned);
    let target_is_group_chat = is_group_chat_target(manager, target_id);
    let mut trpg_membership_selected = trpg_membership_group
        .as_deref()
        .and_then(|group_name| manager.trpg_groups.get(group_name))
        .map(|group| {
            if target_is_group_chat {
                group.group_chats.contains(&target_id.to_owned())
            } else {
                group.players.contains(&target_id.to_owned())
            }
        })
        .unwrap_or_default();
    let mut trpg_membership_changed = false;
    let trpg_turn_snapshot = trpg_membership_group.as_deref().and_then(|group_name| {
        manager
            .trpg_groups
            .get(group_name)
            .filter(|group| {
                !target_is_group_chat
                    && group.players.iter().any(|player_id| player_id == target_id)
            })
            .map(|group| {
                let turn = group.player_turns.get(target_id);
                (
                    group_name.to_owned(),
                    group.world_turn,
                    turn.map(|turn| turn.turns_passed).unwrap_or_default(),
                    turn.map(|turn| turn.acted).unwrap_or_default(),
                    turn.map(|turn| turn.skipped).unwrap_or_default(),
                )
            })
    });
    let mut player_turn_count_set: Option<(String, String, u32)> = None;
    let mut player_acted_toggle: Option<(String, String, bool)> = None;
    let player_visible_options = player_visible_preview_options(manager, target_id, messages);
    let response = window.show(ctx, |ui| {
        if current_group.is_some() || show_character_button || trpg_membership_group.is_some() {
            ui.horizontal(|ui| {
                if let Some(group_name) = trpg_membership_group.as_deref() {
                    if ui
                        .checkbox(
                            &mut trpg_membership_selected,
                            target_display_name(manager, target_id),
                        )
                        .on_hover_text(format!(
                            "切换在{group_name}中的成员状态"
                        ))
                        .changed()
                    {
                        trpg_membership_changed = true;
                    }
                }
                if show_character_button {
                    if ui.button("角色").clicked() {
                        quick_character_targets.insert(target_id.to_owned());
                    }
                    let can_view_player =
                        target_id.parse::<u64>().is_ok() && player_view_request.is_some();
                    if ui
                        .add_enabled(
                            can_view_player,
                            egui::Button::new("查看玩家视角"),
                        )
                        .on_hover_text("切换到这个玩家的场景捕捉相机，并按其可见性过滤场景")
                        .clicked()
                    {
                        if let (Ok(user_id), Some(request)) = (
                            target_id.parse::<u64>(),
                            player_view_request.as_deref_mut(),
                        ) {
                            request.view_with_capture_camera(user_id);
                        }
                    }
                }
                if !player_visible_options.is_empty() {
                    let mut preview_enabled = chat_player_visible_previews.contains_key(target_id);
                    if ui
                        .checkbox(&mut preview_enabled, "按玩家可见")
                        .on_hover_text("按所选玩家的可读范围过滤这个聊天窗口")
                        .changed()
                    {
                        if preview_enabled {
                            chat_player_visible_previews.insert(
                                target_id.to_owned(),
                                player_visible_options[0].clone(),
                            );
                        } else {
                            chat_player_visible_previews.remove(target_id);
                        }
                    }
                    if preview_enabled {
                        let selected = chat_player_visible_previews
                            .entry(target_id.to_owned())
                            .or_insert_with(|| player_visible_options[0].clone());
                        if !player_visible_options
                            .iter()
                            .any(|player_id| player_id == selected)
                        {
                            *selected = player_visible_options[0].clone();
                        }
                        if player_visible_options.len() == 1 {
                            ui.small(target_display_name(manager, selected));
                        } else {
                            egui::ComboBox::from_id_salt((
                                "chat_player_visible_preview",
                                target_id,
                            ))
                            .selected_text(target_display_name(manager, selected))
                            .show_ui(ui, |ui| {
                                for player_id in &player_visible_options {
                                    ui.selectable_value(
                                        selected,
                                        player_id.clone(),
                                        target_display_name(manager, player_id),
                                    );
                                }
                            });
                        }
                    }
                }
                if let Some((group_name, _, _, acted, _)) = trpg_turn_snapshot.as_ref() {
                    let button_text = if *acted { "已行动" } else { "行动" };
                    if ui.button(button_text).clicked() {
                        player_acted_toggle = Some((
                            group_name.clone(),
                            target_id.to_owned(),
                            !*acted,
                        ));
                    }
                }
                ui.with_layout(
                    egui::Layout::right_to_left(egui::Align::Center),
                    |ui| {
                        if current_group.is_some()
                            && ui.button("离开").on_hover_text("离开讨论组").clicked()
                        {
                            leave_group = true;
                        }
                    },
                );
            });
            if let Some((group_name, world_turn, turns_passed, acted, skipped)) =
                trpg_turn_snapshot.as_ref()
            {
                ui.horizontal_wrapped(|ui| {
                    let status = if *acted {
                        "已行动"
                    } else if *skipped {
                        "已跳过"
                    } else {
                        "等待中"
                    };
                    ui.small(format!("世界轮次 {world_turn}"));
                    ui.small(format!("玩家轮次 {turns_passed}"));
                    ui.small(status);

                    let draft_key = (group_name.clone(), target_id.to_owned());
                    let draft = turn_count_drafts
                        .entry(draft_key.clone())
                        .or_insert(*turns_passed);
                    ui.add(
                        egui::DragValue::new(draft)
                            .range(0..=9999)
                            .speed(1)
                            .prefix("设为 "),
                    );
                    if ui.button("设置轮次").clicked() {
                        player_turn_count_set = Some((
                            group_name.clone(),
                            target_id.to_owned(),
                            *draft,
                        ));
                    }
                    if ui.small_button("当前").clicked() {
                        *draft = *turns_passed;
                    }
                });
            }
        }
        let preview_messages = chat_player_visible_previews
            .get(target_id)
            .and_then(|player_id| player_id.parse::<u64>().ok())
            .map(|player_id| manager.visible_messages_for_player(target_id, messages, player_id));
        if let Some(preview_messages) = preview_messages.as_ref() {
            ui.small(format!(
                "玩家可见消息 {}/{}",
                preview_messages.len(),
                messages.len()
            ));
        }
        let body_messages = preview_messages.as_ref().unwrap_or(messages);
        chat_body_ui(
            ui,
            ctx,
            body_messages,
            napcat_sender,
            target_id,
            chat_input_msgs,
            targets,
            ime,
            chat_scroll_states,
            image_textures,
            None,
        );
    });

    if let Some((group_name, target_id, turns_passed)) = player_turn_count_set {
        if manager
            .trpg_groups
            .get_mut(&group_name)
            .is_some_and(|group| group.set_player_turns_passed(&target_id, turns_passed))
        {
            manager.persist().ok();
        }
    }

    if let Some((group_name, target_id, acted)) = player_acted_toggle {
        let changed = if acted {
            mark_group_player_turn(
                manager.as_mut(),
                &group_name,
                &target_id,
                true,
                rule_engine_state,
            )
        } else {
            set_group_player_waiting(
                manager.as_mut(),
                &group_name,
                &target_id,
            )
        };
        if changed {
            manager.persist().ok();
        }
    }

    if trpg_membership_changed {
        if let Some(group_name) = trpg_membership_group.as_deref() {
            if let Some(group) = manager.trpg_groups.get_mut(group_name) {
                if target_is_group_chat {
                    set_target_membership(
                        &mut group.group_chats,
                        target_id,
                        trpg_membership_selected,
                    );
                } else {
                    set_target_membership(
                        &mut group.players,
                        target_id,
                        trpg_membership_selected,
                    );
                    group.sync_turn_players();
                    group.sync_legacy_negative_timers();
                }
                manager.persist().ok();
            }
        }
    }

    if let Some(group_name) = current_group {
        ctx.set_sublayer(
            egui::LayerId::new(egui::Order::Middle, Id::new(group_name)),
            egui::LayerId::new(egui::Order::Middle, window_id),
        );
    }

    if current_group.is_some() && !window_open {
        leave_group = true;
    }
    if current_group.is_none() && !window_open {
        manager.open_chat_targets.remove(target_id);
        manager.persist().ok();
        return;
    }
    if let Some(group_name) = current_group {
        if leave_group {
            if let Some(group) = manager.groups.get_mut(group_name) {
                group.members.retain(|member_id| member_id != target_id);
                manager.persist().ok();
            }
            return;
        }
    }

    if let Some(response) = response {
        if current_group.is_none() {
            paint_unread_badge(
                ctx,
                response.response.rect,
                unread_count,
            );
        }

        if window_received_focus(ctx, &response.response) {
            mark_target_read(manager, target_id, messages.len());
        }

        if current_group.is_some() {
            return;
        }

        if let Some(drop_pos) = ctx.input(|input| input.pointer.latest_pos()) {
            if response.response.dragged() {
                if let Some((_, preview_rect)) =
                    group_rects.iter().find(|(_, rect)| rect.contains(drop_pos))
                {
                    draw_drop_preview(ctx, *preview_rect);
                }
            }
        }

        if !response.response.drag_stopped() {
            return;
        }

        let Some(drop_pos) = ctx.input(|input| input.pointer.latest_pos()) else {
            return;
        };

        for (k, rect) in group_rects {
            if rect.contains(drop_pos) {
                let Some(members) = manager.groups.get_mut(k).map(|group| &mut group.members)
                else {
                    continue;
                };
                if !members.contains(&target_id.to_owned()) {
                    members.push(target_id.to_string());
                    manager.persist().ok();
                }
            }
        }
    }
}

fn player_visible_preview_options(
    manager: &NapcatMessageManager,
    target_id: &str,
    messages: &[NapcatMessage],
) -> Vec<String> {
    match target_kind_with_messages(manager, target_id, messages) {
        ChatTargetExportKind::Private => target_id
            .parse::<u64>()
            .ok()
            .map(|_| vec![target_id.to_owned()])
            .unwrap_or_default(),
        ChatTargetExportKind::Group => manager
            .current_group()
            .filter(|group| {
                group
                    .group_chats
                    .iter()
                    .any(|group_id| group_id == target_id)
            })
            .map(|group| {
                let mut players = group
                    .players
                    .iter()
                    .filter(|player_id| player_id.parse::<u64>().is_ok())
                    .cloned()
                    .collect::<Vec<_>>();
                players.sort();
                players.dedup();
                players
            })
            .unwrap_or_default(),
        ChatTargetExportKind::Unknown => Vec::new(),
    }
}

fn window_received_focus(ctx: &Context, response: &Response) -> bool {
    response.contains_pointer() && ctx.input(|input| input.pointer.any_pressed())
}

fn standalone_chat_window_id(id: Id, target_id: &str) -> Id {
    Id::new((
        id,
        target_id,
        "standalone_chat_window_v2",
    ))
}

fn focus_standalone_chat_window(ctx: &Context, target_id: &str) {
    let window_id = standalone_chat_window_id(Id::new(target_id), target_id);
    let mut collapsing = egui::collapsing_header::CollapsingState::load_with_default_open(
        ctx,
        window_id.with("collapsing"),
        true,
    );
    collapsing.set_open(true);
    collapsing.store(ctx);
    ctx.move_to_top(egui::LayerId::new(
        egui::Order::Middle,
        window_id,
    ));
    ctx.request_repaint();
}

fn mark_target_read(
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    target_id: &str,
    message_count: usize,
) {
    let current_read_count = manager
        .read_message_counts
        .get(target_id)
        .copied()
        .unwrap_or_default();
    if current_read_count >= message_count {
        return;
    }

    manager
        .read_message_counts
        .insert(target_id.to_owned(), message_count);
    manager.persist().ok();
}

fn group_member_constraint_rect(rect: Rect) -> Rect {
    let min = egui::pos2(
        rect.left() + GROUP_MEMBER_WINDOW_SIDE_GAP,
        rect.top() + GROUP_MEMBER_WINDOW_TOP_GAP,
    );
    let max = egui::pos2(
        rect.right() - GROUP_MEMBER_WINDOW_SIDE_GAP,
        rect.bottom() - GROUP_MEMBER_WINDOW_BOTTOM_GAP,
    );

    if max.x > min.x + CHAT_WINDOW_MIN_SIZE.x && max.y > min.y + CHAT_WINDOW_MIN_SIZE.y {
        Rect::from_min_max(min, max)
    } else {
        rect.shrink2(egui::vec2(
            GROUP_MEMBER_WINDOW_SIDE_GAP.min(rect.width() * 0.25),
            GROUP_MEMBER_WINDOW_SIDE_GAP.min(rect.height() * 0.25),
        ))
    }
}

fn group_member_default_pos(rect: Rect, target_id: &str) -> Pos2 {
    let mut hasher = DefaultHasher::new();
    target_id.hash(&mut hasher);
    let hash = hasher.finish();
    let x_slots = ((rect.width() - CHAT_WINDOW_SIZE.x).max(0.0) / 36.0).floor() as u64 + 1;
    let y_slots = ((rect.height() - CHAT_WINDOW_SIZE.y).max(0.0) / 36.0).floor() as u64 + 1;
    let x = rect.left() + 12.0 + (hash % x_slots) as f32 * 36.0;
    let y = rect.top() + 12.0 + ((hash / 17) % y_slots) as f32 * 36.0;
    egui::pos2(x, y)
}

fn chat_body_ui(
    ui: &mut Ui,
    ctx: &Context,
    messages: &Vec<NapcatMessage>,
    napcat_sender: Option<&NapcatIOSender>,
    target_id: &str,
    chat_input_msgs: &mut Local<HashMap<String, String>>,
    targets: Vec<NapcatSendTarget>,
    ime: &mut ResMut<ImeManager>,
    chat_scroll_states: &mut Local<HashMap<String, ChatScrollState>>,
    image_textures: &mut Local<HashMap<String, TextureHandle>>,
    desired_height: Option<f32>,
) {
    ui.vertical(|ui| {
        if !chat_input_msgs.contains_key(target_id) {
            chat_input_msgs.insert(target_id.to_owned(), String::new());
        }

        let input_height = ui.spacing().interact_size.y * 3.0 + ui.spacing().item_spacing.y * 2.0;
        let available_height = desired_height.unwrap_or_else(|| ui.available_height());
        let message_height =
            (available_height - input_height - ui.spacing().item_spacing.y).max(0.0);

        let message_width = ui.available_width();
        ui.allocate_ui(
            egui::vec2(message_width, message_height),
            |ui| {
                let scroll_state = chat_scroll_states
                    .entry(target_id.to_owned())
                    .or_insert_with(|| ChatScrollState {
                        message_count: messages.len(),
                        near_bottom: true,
                    });
                let should_stick_to_bottom =
                    messages.len() > scroll_state.message_count && scroll_state.near_bottom;
                let mut scroll_area = egui::ScrollArea::vertical()
                    .id_salt((target_id, "messages"))
                    .max_height(message_height)
                    .min_scrolled_height(message_height)
                    .auto_shrink([false, false]);
                if should_stick_to_bottom {
                    scroll_area = scroll_area.stick_to_bottom(true);
                }

                let output = scroll_area.show(ui, |ui| {
                    ui.with_layout(
                        egui::Layout::top_down(egui::Align::LEFT),
                        |ui| {
                            for message in messages {
                                message_row_ui(
                                    ui,
                                    message,
                                    message_width,
                                    image_textures,
                                );
                                ui.add_space(ui.spacing().item_spacing.y);
                            }
                        },
                    );
                });

                let max_scroll_y = (output.content_size.y - output.inner_rect.height()).max(0.0);
                let distance_to_bottom = (max_scroll_y - output.state.offset.y).max(0.0);
                scroll_state.message_count = messages.len();
                scroll_state.near_bottom =
                    should_stick_to_bottom || distance_to_bottom <= CHAT_AUTO_SCROLL_THRESHOLD;
            },
        );

        ui.add_space(ui.spacing().item_spacing.y);
        let text = chat_input_msgs.get_mut(target_id).unwrap();
        if let Some(napcat_sender) = napcat_sender {
            let _ = ime.chat_input_multiline(
                target_id,
                text,
                ui.available_width(),
                3,
                ui,
                ctx,
                napcat_sender,
                targets,
            );
        } else {
            ui.add_enabled(
                false,
                egui::TextEdit::multiline(text)
                    .desired_width(ui.available_width())
                    .desired_rows(3),
            );
        }
    });
}

fn group_broadcast_input_ui(
    ui: &mut Ui,
    ctx: &Context,
    group_name: &str,
    members: &[String],
    manager: &NapcatMessageManager,
    napcat_sender: Option<&NapcatIOSender>,
    chat_input_msgs: &mut Local<HashMap<String, String>>,
    broadcast_scopes: &mut Local<HashMap<String, String>>,
    ime: &mut ResMut<ImeManager>,
) {
    let input_id = format!("group:{group_name}:broadcast");
    chat_input_msgs
        .entry(input_id.clone())
        .or_insert_with(String::new);

    ui.separator();
    let text = chat_input_msgs.get_mut(&input_id).unwrap();
    let current_group = manager.current_group();
    let scope = broadcast_scopes
        .entry(group_name.to_owned())
        .or_insert_with(|| BROADCAST_SCOPE_ALL.to_owned());
    group_broadcast_scope_ui(
        ui,
        group_name,
        members,
        current_group,
        scope,
    );
    let targets = group_broadcast_targets(current_group, members, manager, scope);

    let enabled = napcat_sender.is_some() && !targets.is_empty();
    if let Some(napcat_sender) = napcat_sender.filter(|_| enabled) {
        let _ = ime.chat_input_multiline(
            &input_id,
            text,
            ui.available_width(),
            GROUP_BROADCAST_INPUT_ROWS,
            ui,
            ctx,
            napcat_sender,
            targets,
        );
    } else {
        ui.add_enabled(
            false,
            egui::TextEdit::multiline(text)
                .desired_width(ui.available_width())
                .desired_rows(GROUP_BROADCAST_INPUT_ROWS),
        );
    }
}

const BROADCAST_SCOPE_ALL: &str = "all";
const BROADCAST_SCOPE_PARTY_PREFIX: &str = "party:";
const BROADCAST_SCOPE_LEGACY_PANE_PREFIX: &str = "legacy-pane:";

fn group_broadcast_scope_ui(
    ui: &mut Ui,
    group_name: &str,
    members: &[String],
    current_group: Option<&TrpgGroup>,
    scope: &mut String,
) {
    let mut party_names = current_group
        .map(|group| {
            group
                .parties
                .keys()
                .filter(|party_id| {
                    members.iter().any(|member_id| {
                        group.party_id_for_player(member_id) == Some(party_id.as_str())
                    })
                })
                .cloned()
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    party_names.sort();
    let mut legacy_panes = current_group
        .map(|group| {
            group
                .legacy_send_panes
                .iter()
                .filter(|pane| !group.legacy_send_pane_members(&pane.key).is_empty())
                .map(|pane| (pane.key.clone(), pane.title.clone()))
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    legacy_panes.sort_by(|left, right| left.1.cmp(&right.1).then(left.0.cmp(&right.0)));

    if scope != BROADCAST_SCOPE_ALL
        && !party_names
            .iter()
            .any(|party_id| scope == &broadcast_party_scope(party_id))
        && !legacy_panes
            .iter()
            .any(|(pane_key, _)| scope == &broadcast_legacy_pane_scope(pane_key))
    {
        *scope = BROADCAST_SCOPE_ALL.to_owned();
    }

    ui.horizontal_wrapped(|ui| {
        ui.label("发送范围");
        egui::ComboBox::from_id_salt((group_name, "broadcast_scope"))
            .selected_text(broadcast_scope_label(
                current_group,
                scope,
            ))
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    scope,
                    BROADCAST_SCOPE_ALL.to_owned(),
                    "全部成员",
                );
                for party_name in party_names {
                    ui.selectable_value(
                        scope,
                        broadcast_party_scope(&party_name),
                        format!("小队：{party_name}"),
                    );
                }
                for (pane_key, pane_title) in legacy_panes {
                    ui.selectable_value(
                        scope,
                        broadcast_legacy_pane_scope(&pane_key),
                        format!("旧发送窗：{pane_title}"),
                    );
                }
            });
    });
}

fn group_broadcast_targets(
    current_group: Option<&TrpgGroup>,
    members: &[String],
    manager: &NapcatMessageManager,
    scope: &str,
) -> Vec<NapcatSendTarget> {
    if let (Some(group), Some(pane_key)) = (
        current_group,
        scope.strip_prefix(BROADCAST_SCOPE_LEGACY_PANE_PREFIX),
    ) {
        let pane_members = group.legacy_send_pane_members(pane_key);
        return private_targets_for_member_ids(manager, pane_members.iter());
    }

    let requested_party = scope.strip_prefix(BROADCAST_SCOPE_PARTY_PREFIX);
    private_targets_for_member_ids(
        manager,
        members.iter().filter(|member_id| match requested_party {
            Some(party_id) => {
                current_group.and_then(|group| group.party_id_for_player(member_id))
                    == Some(party_id)
            },
            None => true,
        }),
    )
}

fn private_targets_for_member_ids<'a>(
    manager: &NapcatMessageManager,
    member_ids: impl Iterator<Item = &'a String>,
) -> Vec<NapcatSendTarget> {
    let mut seen = HashSet::new();
    let mut targets = member_ids
        .filter_map(|member_id| private_broadcast_target(manager, member_id))
        .filter(|target| match target {
            NapcatSendTarget::Private(user_id) => seen.insert(*user_id),
            NapcatSendTarget::Group(_) => false,
        })
        .collect::<Vec<_>>();
    targets.sort_by_key(|target| match target {
        NapcatSendTarget::Private(user_id) => *user_id,
        NapcatSendTarget::Group(group_id) => *group_id,
    });
    targets
}

fn private_broadcast_target(
    manager: &NapcatMessageManager,
    member_id: &str,
) -> Option<NapcatSendTarget> {
    if manager.chat_target_kind(member_id) != ChatTargetExportKind::Private {
        return None;
    }

    match member_id.parse::<u64>() {
        Ok(user_id) => Some(NapcatSendTarget::Private(user_id)),
        Err(_) => {
            eprintln!("invalid NapCat group member id: {member_id}");
            None
        },
    }
}

fn broadcast_party_scope(party_id: &str) -> String {
    format!("{BROADCAST_SCOPE_PARTY_PREFIX}{party_id}")
}

fn broadcast_legacy_pane_scope(pane_key: &str) -> String {
    format!("{BROADCAST_SCOPE_LEGACY_PANE_PREFIX}{pane_key}")
}

fn broadcast_scope_label(current_group: Option<&TrpgGroup>, scope: &str) -> String {
    if let Some(party_id) = scope.strip_prefix(BROADCAST_SCOPE_PARTY_PREFIX) {
        return format!("小队：{party_id}");
    }
    if let Some(pane_key) = scope.strip_prefix(BROADCAST_SCOPE_LEGACY_PANE_PREFIX) {
        if let Some(pane) = current_group.and_then(|group| group.legacy_send_pane(pane_key)) {
            return format!("旧发送窗：{}", pane.title);
        }
        return format!("旧发送窗：{pane_key}");
    }
    "全部成员".to_owned()
}

fn legacy_send_pane_input_id(group_name: &str, pane_key: &str) -> String {
    format!("legacy-send-pane:{group_name}:{pane_key}")
}

fn legacy_team_chat_input_id(group_name: &str, team_id: &str) -> String {
    format!("legacy-team-chat:{group_name}:{team_id}")
}

fn legacy_team_chat_edit_prefix(group_name: &str, team_id: &str) -> String {
    format!("legacy-team-chat-edit:{group_name}:{team_id}:")
}

fn legacy_team_chat_edit_id(group_name: &str, team_id: &str, message_index: usize) -> String {
    format!(
        "{}{message_index}",
        legacy_team_chat_edit_prefix(group_name, team_id)
    )
}

fn legacy_send_pane_targets(
    manager: &NapcatMessageManager,
    group: &TrpgGroup,
    pane_key: &str,
) -> Vec<NapcatSendTarget> {
    group_broadcast_targets(
        Some(group),
        &group.players,
        manager,
        &broadcast_legacy_pane_scope(pane_key),
    )
}

fn queue_legacy_send_pane_text(
    manager: &NapcatMessageManager,
    group: &TrpgGroup,
    group_name: &str,
    pane_key: &str,
    text: &str,
    sender: &NapcatIOSender,
    ime: &mut ImeManager,
) -> Result<usize, String> {
    if text.trim().is_empty() {
        return Err("发送内容为空".to_owned());
    }

    let targets = legacy_send_pane_targets(manager, group, pane_key);
    let target_count = targets.len();
    ime.queue_text_send(
        &legacy_send_pane_input_id(group_name, pane_key),
        text,
        sender,
        targets,
    )?;
    Ok(target_count)
}

fn legacy_team_chat_targets(
    manager: &NapcatMessageManager,
    group: &TrpgGroup,
    team_id: &str,
) -> Vec<NapcatSendTarget> {
    group
        .legacy_team_members(team_id)
        .into_iter()
        .filter_map(|target_id| {
            let user_id = target_id.parse::<u64>().ok()?;
            manager
                .messages
                .contains_key(&target_id)
                .then_some(NapcatSendTarget::Private(user_id))
        })
        .collect()
}

fn legacy_team_chat_message(text: &str, time: u64) -> TrpgLegacyTeamChatMessage {
    TrpgLegacyTeamChatMessage {
        sender_id: "gm".to_owned(),
        sender_name: "GM".to_owned(),
        text: text.trim().to_owned(),
        time,
    }
}

fn queue_legacy_team_chat_text(
    manager: &NapcatMessageManager,
    group: &TrpgGroup,
    group_name: &str,
    team_id: &str,
    text: &str,
    sender: &NapcatIOSender,
    ime: &mut ImeManager,
) -> Result<usize, String> {
    if text.trim().is_empty() {
        return Err("发送内容为空".to_owned());
    }

    let targets = legacy_team_chat_targets(manager, group, team_id);
    let target_count = targets.len();
    ime.queue_text_send(
        &legacy_team_chat_input_id(group_name, team_id),
        text,
        sender,
        targets,
    )?;
    Ok(target_count)
}

fn legacy_team_chat_title(team: &crate::napcat::TrpgLegacyTeam) -> String {
    let name = team.name.trim();
    if !name.is_empty() {
        return name.to_owned();
    }
    let id = team.id.trim();
    if !id.is_empty() {
        return format!("旧频道 {id}");
    }
    "旧频道".to_owned()
}

fn legacy_team_chat_default_size(team: &crate::napcat::TrpgLegacyTeam) -> Vec2 {
    if team.window_width > 0.0 && team.window_height > 0.0 {
        return Vec2::new(
            team.window_width.max(320.0),
            team.window_height.max(260.0),
        );
    }
    Vec2::new(420.0, 360.0)
}

fn legacy_team_chat_default_pos(team: &crate::napcat::TrpgLegacyTeam) -> Option<Pos2> {
    (team.window_x != 0.0 || team.window_y != 0.0).then(|| Pos2::new(team.window_x, team.window_y))
}

fn legacy_team_chat_composer_ui(
    ui: &mut Ui,
    manager: &NapcatMessageManager,
    group: &TrpgGroup,
    group_name: &str,
    team: &crate::napcat::TrpgLegacyTeam,
    state: &mut TrpgGroupSettingsState,
    chat_input_msgs: &mut HashMap<String, String>,
    napcat_sender: Option<&NapcatIOSender>,
    ime: &mut ImeManager,
    desired_rows: usize,
    scroll_height: f32,
) -> Option<LegacyGroupSurfaceAction> {
    let mut action = None;
    if !team.chat_messages.is_empty() {
        egui::ScrollArea::vertical()
            .id_salt((group_name, "legacy_team_chat", &team.id))
            .max_height(scroll_height)
            .show(ui, |ui| {
                for (message_index, message) in team.chat_messages.iter().enumerate() {
                    let sender = legacy_team_chat_sender_label(
                        manager,
                        &message.sender_id,
                        &message.sender_name,
                    );
                    let draft_key = legacy_team_chat_edit_id(group_name, &team.id, message_index);
                    let draft = state
                        .legacy_team_chat_edit_drafts
                        .entry(draft_key)
                        .or_insert_with(|| message.text.clone());
                    ui.small(if message.time > 0 {
                        format!("{sender} · {}", message.time)
                    } else {
                        sender
                    });
                    ui.add(
                        egui::TextEdit::multiline(draft)
                            .desired_rows(2)
                            .desired_width(ui.available_width()),
                    );
                    let save_enabled =
                        !draft.trim().is_empty() && draft.as_str() != message.text.as_str();
                    let save_text = draft.clone();
                    ui.horizontal_wrapped(|ui| {
                        let save_response = ui.add_enabled(save_enabled, egui::Button::new("保存"));
                        if save_response.clicked() {
                            action = Some(
                                LegacyGroupSurfaceAction::UpdateTeamChat {
                                    team_id: team.id.clone(),
                                    message_index,
                                    text: save_text.clone(),
                                },
                            );
                        }
                        save_response.on_hover_text("保存这条本地旧频道消息");
                        if ui.button("删除").clicked() {
                            action = Some(
                                LegacyGroupSurfaceAction::RemoveTeamChat {
                                    team_id: team.id.clone(),
                                    message_index,
                                },
                            );
                        }
                    });
                    ui.separator();
                }
            });
    }
    let unparsed_count = team
        .chat_message_count
        .saturating_sub(team.chat_messages.len());
    if unparsed_count > 0 {
        ui.small(format!(
            "另有 {}条旧消息没有可预览文本",
            unparsed_count
        ));
    }
    let input_id = legacy_team_chat_input_id(group_name, &team.id);
    chat_input_msgs
        .entry(input_id.clone())
        .or_insert_with(String::new);
    let targets = legacy_team_chat_targets(manager, group, &team.id);
    ui.small(format!(
        "可发送 {}人：{}",
        targets.len(),
        legacy_member_preview(
            manager,
            &group.legacy_team_members(&team.id)
        )
    ));
    let text = chat_input_msgs.get_mut(&input_id).unwrap();
    ui.add(
        egui::TextEdit::multiline(text)
            .desired_rows(desired_rows)
            .desired_width(ui.available_width()),
    );
    let text_to_send = text.trim().to_owned();
    let send_enabled = napcat_sender.is_some() && !targets.is_empty() && !text_to_send.is_empty();
    let response = ui.add_enabled(
        send_enabled,
        egui::Button::new("发送到旧频道"),
    );
    let clicked = response.clicked();
    let hover_text = if napcat_sender.is_none() {
        "NapCat websocket未连接"
    } else if targets.is_empty() {
        "这个旧频道没有可发送的玩家私聊"
    } else if text_to_send.is_empty() {
        "输入内容后发送"
    } else {
        "发送到这个旧频道的玩家私聊，并追加到本地旧频道聊天"
    };
    response.on_hover_text(hover_text);
    if clicked {
        if let Some(sender) = napcat_sender {
            match queue_legacy_team_chat_text(
                manager,
                group,
                group_name,
                &team.id,
                &text_to_send,
                sender,
                ime,
            ) {
                Ok(target_count) => {
                    let time = SystemTime::now()
                        .duration_since(UNIX_EPOCH)
                        .map(|duration| duration.as_secs())
                        .unwrap_or_default();
                    action = Some(
                        LegacyGroupSurfaceAction::AppendTeamChat {
                            team_id: team.id.clone(),
                            message: legacy_team_chat_message(&text_to_send, time),
                        },
                    );
                    state.legacy_team_chat_status.insert(
                        input_id.clone(),
                        format!("已入队：{}个目标", target_count),
                    );
                },
                Err(err) => {
                    state.legacy_team_chat_status.insert(
                        input_id.clone(),
                        format!("发送失败：{err}"),
                    );
                },
            }
        }
    }
    if let Some(status) = state.legacy_team_chat_status.get(&input_id) {
        ui.small(status);
    }
    action
}

#[derive(Debug, Clone, PartialEq)]
struct LegacyTeamChatWindowEntry {
    group_name: String,
    team_id: String,
    title: String,
    visible: bool,
    default_pos: Option<Pos2>,
    default_size: Vec2,
}

fn legacy_team_chat_window_entries(
    manager: &NapcatMessageManager,
    open_windows: &HashSet<(String, String)>,
) -> Vec<LegacyTeamChatWindowEntry> {
    let mut entries = Vec::new();
    for (group_name, group) in &manager.trpg_groups {
        for team in &group.legacy_teams {
            let key = (group_name.clone(), team.id.clone());
            if !open_windows.contains(&key) {
                continue;
            }
            entries.push(LegacyTeamChatWindowEntry {
                group_name: group_name.clone(),
                team_id: team.id.clone(),
                title: legacy_team_chat_title(team),
                visible: team.visible,
                default_pos: legacy_team_chat_default_pos(team),
                default_size: legacy_team_chat_default_size(team),
            });
        }
    }
    entries.sort_by(|left, right| {
        left.group_name
            .cmp(&right.group_name)
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.team_id.cmp(&right.team_id))
    });
    entries
}

fn legacy_team_chat_windows(
    ctx: &Context,
    manager: &NapcatMessageManager,
    napcat_sender: Option<&NapcatIOSender>,
    chat_input_msgs: &mut HashMap<String, String>,
    state: &mut TrpgGroupSettingsState,
    ime: &mut ImeManager,
) -> Option<(String, LegacyGroupSurfaceAction)> {
    let mut action = None;
    let entries = legacy_team_chat_window_entries(
        manager,
        &state.open_legacy_team_chat_windows,
    );
    for entry in entries {
        let Some(group) = manager.trpg_groups.get(&entry.group_name) else {
            continue;
        };
        let Some(team) = group.legacy_team(&entry.team_id) else {
            continue;
        };

        let mut window_open = true;
        let mut window = egui::Window::new(format!(
            "旧频道聊天：{} / {}",
            entry.group_name, entry.title
        ))
        .id(Id::new((
            "legacy_team_chat_window",
            entry.group_name.as_str(),
            entry.team_id.as_str(),
        )))
        .default_size(entry.default_size)
        .min_width(300.0)
        .open(&mut window_open);
        if let Some(default_pos) = entry.default_pos {
            window = window.default_pos(default_pos);
        }
        window.show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.strong(&entry.title);
                ui.small(format!(
                    "本地聊天 {}条",
                    team.chat_message_count
                ));
                ui.small(format!(
                    "成员：{}",
                    legacy_member_preview(manager, &team.players)
                ));
                if !entry.visible {
                    ui.small("旧频道隐藏");
                }
            });
            if let Some(next_action) = legacy_team_chat_composer_ui(
                ui,
                manager,
                group,
                &entry.group_name,
                team,
                state,
                chat_input_msgs,
                napcat_sender,
                ime,
                3,
                220.0,
            ) {
                action = Some((entry.group_name.clone(), next_action));
            }
        });
        if !window_open {
            state
                .open_legacy_team_chat_windows
                .remove(&(entry.group_name, entry.team_id));
        }
    }
    action
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct LegacySendPaneWindowEntry {
    group_name: String,
    pane_key: String,
    title: String,
    closable: bool,
}

fn legacy_send_pane_window_entries(
    manager: &NapcatMessageManager,
    open_windows: &HashSet<(String, String)>,
) -> Vec<LegacySendPaneWindowEntry> {
    let mut entries = Vec::new();
    for (group_name, group) in &manager.trpg_groups {
        for pane in &group.legacy_send_panes {
            let key = (group_name.clone(), pane.key.clone());
            if pane.closable && !open_windows.contains(&key) {
                continue;
            }
            entries.push(LegacySendPaneWindowEntry {
                group_name: group_name.clone(),
                pane_key: pane.key.clone(),
                title: pane.title.clone(),
                closable: pane.closable,
            });
        }
    }
    entries.sort_by(|left, right| {
        left.group_name
            .cmp(&right.group_name)
            .then_with(|| left.title.cmp(&right.title))
            .then_with(|| left.pane_key.cmp(&right.pane_key))
    });
    entries
}

fn legacy_send_pane_composer_ui(
    ui: &mut Ui,
    manager: &NapcatMessageManager,
    group: &TrpgGroup,
    group_name: &str,
    pane: &TrpgLegacySendPane,
    napcat_sender: Option<&NapcatIOSender>,
    chat_input_msgs: &mut HashMap<String, String>,
    status: &mut HashMap<String, String>,
    ime: &mut ImeManager,
    desired_rows: usize,
) {
    let resolved = group.legacy_send_pane_members(&pane.key);
    let targets = legacy_send_pane_targets(manager, group, &pane.key);
    let effective_targets = group.legacy_send_pane_effective_targets(&pane.key);
    let disabled_direct_targets = group.legacy_send_pane_disabled_direct_targets(&pane.key);
    let input_id = legacy_send_pane_input_id(group_name, &pane.key);
    chat_input_msgs
        .entry(input_id.clone())
        .or_insert_with(String::new);

    ui.horizontal_wrapped(|ui| {
        ui.strong(&pane.title);
        ui.small(format!("目标 {}项", pane.targets.len()));
        if effective_targets.len() != pane.targets.len() {
            ui.small(format!(
                "有效 {}项",
                effective_targets.len()
            ));
        }
        ui.small(format!("可发送 {}人", targets.len()));
        ui.small(format!(
            "成员：{}",
            legacy_member_preview(manager, &resolved)
        ));
        if !disabled_direct_targets.is_empty() {
            ui.small(format!(
                "重复PC已禁用：{}",
                legacy_member_preview(manager, &disabled_direct_targets)
            ));
        }
        if !pane.closable {
            ui.small("旧窗固定");
        }
    });

    let text = chat_input_msgs.get_mut(&input_id).unwrap();
    ui.add(
        egui::TextEdit::multiline(text)
            .desired_rows(desired_rows)
            .desired_width(ui.available_width()),
    );
    let text_to_send = text.trim().to_owned();
    let send_enabled = napcat_sender.is_some() && !targets.is_empty() && !text_to_send.is_empty();
    ui.horizontal_wrapped(|ui| {
        let response = ui.add_enabled(send_enabled, egui::Button::new("发送"));
        let clicked = response.clicked();
        let hover_text = if napcat_sender.is_none() {
            "NapCat websocket未连接"
        } else if targets.is_empty() {
            "这个旧发送窗没有可发送的玩家私聊"
        } else if text_to_send.is_empty() {
            "输入内容后发送"
        } else {
            "发送到这个旧发送窗的解析目标"
        };
        response.on_hover_text(hover_text);
        if clicked {
            if let Some(sender) = napcat_sender {
                match queue_legacy_send_pane_text(
                    manager,
                    group,
                    group_name,
                    &pane.key,
                    &text_to_send,
                    sender,
                    ime,
                ) {
                    Ok(target_count) => {
                        status.insert(
                            input_id.clone(),
                            format!("已入队：{}个目标", target_count),
                        );
                    },
                    Err(err) => {
                        status.insert(
                            input_id.clone(),
                            format!("发送失败：{err}"),
                        );
                    },
                }
            }
        }
        if let Some(status_text) = status.get(&input_id) {
            ui.small(status_text);
        }
    });
}

fn legacy_send_pane_windows(
    ctx: &Context,
    manager: &NapcatMessageManager,
    napcat_sender: Option<&NapcatIOSender>,
    chat_input_msgs: &mut HashMap<String, String>,
    state: &mut TrpgGroupSettingsState,
    ime: &mut ImeManager,
) {
    let entries = legacy_send_pane_window_entries(
        manager,
        &state.open_legacy_send_pane_windows,
    );
    for entry in entries {
        let Some(group) = manager.trpg_groups.get(&entry.group_name) else {
            continue;
        };
        let Some(pane) = group.legacy_send_pane(&entry.pane_key) else {
            continue;
        };

        let mut window_open = true;
        let mut window = egui::Window::new(format!(
            "旧发送窗：{} / {}",
            entry.group_name, entry.title
        ))
        .id(Id::new((
            "legacy_send_pane_window",
            entry.group_name.as_str(),
            entry.pane_key.as_str(),
        )))
        .default_size(Vec2::new(360.0, 180.0))
        .min_width(280.0);
        if entry.closable {
            window = window.open(&mut window_open);
        }
        window.show(ctx, |ui| {
            legacy_send_pane_composer_ui(
                ui,
                manager,
                group,
                &entry.group_name,
                pane,
                napcat_sender,
                chat_input_msgs,
                &mut state.legacy_send_pane_status,
                ime,
                4,
            );
        });
        if entry.closable && !window_open {
            state
                .open_legacy_send_pane_windows
                .remove(&(entry.group_name, entry.pane_key));
        }
    }
}

fn group_drop_area_ui(ui: &mut Ui, group_name: &str, members: &[String]) {
    let body_height =
        (ui.available_height() - GROUP_BROADCAST_INPUT_HEIGHT - ui.spacing().item_spacing.y)
            .max(GROUP_CHAT_MIN_HEIGHT);
    let (rect, _) = ui.allocate_exact_size(
        egui::vec2(ui.available_width(), body_height),
        Sense::hover(),
    );

    if ui.is_rect_visible(rect) {
        let painter = ui.painter();
        painter.rect_stroke(
            rect,
            4.0,
            Stroke::new(
                1.0,
                ui.visuals().widgets.noninteractive.bg_stroke.color,
            ),
            egui::epaint::StrokeKind::Inside,
        );
        if members.is_empty() {
            painter.text(
                rect.center(),
                egui::Align2::CENTER_CENTER,
                format!("拖入聊天到讨论组 {group_name}"),
                egui::TextStyle::Body.resolve(ui.style()),
                ui.visuals().weak_text_color(),
            );
        }
    }
}

fn message_row_ui(
    ui: &mut Ui,
    message: &NapcatMessage,
    row_width: f32,
    image_textures: &mut Local<HashMap<String, TextureHandle>>,
) {
    let is_self = message.data.self_id == message.data.user_id;
    let max_message_width = if row_width < 120.0 {
        row_width
    } else {
        (row_width * 0.72).clamp(120.0, row_width)
    };
    let margin_width = (row_width - max_message_width).max(0.0);

    ui.horizontal_top(|ui| {
        ui.set_width(row_width);
        if is_self {
            ui.add_space(margin_width);
            ui.vertical(|ui| {
                ui.set_width(max_message_width);
                ui.set_max_width(max_message_width);
                ui.with_layout(
                    egui::Layout::top_down(egui::Align::RIGHT),
                    |ui| {
                        message_text_ui(ui, message, image_textures);
                    },
                );
            });
        } else {
            ui.vertical(|ui| {
                ui.set_width(max_message_width);
                ui.set_max_width(max_message_width);
                message_text_ui(ui, message, image_textures);
            });
            ui.add_space(margin_width);
        }
    });
}

fn message_text_ui(
    ui: &mut Ui,
    message: &NapcatMessage,
    image_textures: &mut Local<HashMap<String, TextureHandle>>,
) {
    ui.label(&message.data.sender.nickname);
    for chain in &message.data.message {
        match &chain.variant {
            NapcatMessageChainType::Text { data: text_data } => {
                ui.add(
                    egui::Label::new(text_data.text.trim())
                        .wrap()
                        .selectable(false),
                );
            },
            NapcatMessageChainType::Image { data } => {
                message_image_ui(ui, data, image_textures);
            },
            NapcatMessageChainType::Source(_) => {},
            NapcatMessageChainType::Unsupported => {},
        }
    }
}

fn message_image_ui(
    ui: &mut Ui,
    data: &ImageData,
    image_textures: &mut Local<HashMap<String, TextureHandle>>,
) {
    let Some(path) = cached_image_path(data.local_path.trim()) else {
        ui.label("[图片]");
        if !data.url.trim().is_empty() {
            ui.small("图片URL不可用");
        }
        return;
    };

    let texture = if let Some(texture) = image_textures.get(&path) {
        texture.clone()
    } else {
        let Some(color_image) = load_cached_color_image(&path) else {
            ui.label("[图片]");
            ui.small("缓存图片解码失败");
            return;
        };
        let texture = ui.ctx().load_texture(
            format!("chat-image:{path}"),
            color_image,
            egui::TextureOptions::LINEAR,
        );
        image_textures.insert(path.clone(), texture.clone());
        texture
    };

    let max_width = ui.available_width().min(CHAT_IMAGE_MAX_SIZE.x).max(1.0);
    let size = fit_image_size(
        texture.size_vec2(),
        Vec2::new(max_width, CHAT_IMAGE_MAX_SIZE.y),
    );
    ui.add(egui::Image::from_texture((texture.id(), size)).corner_radius(4))
        .on_hover_text(data.url.trim());
}

fn cached_image_path(path: &str) -> Option<String> {
    if path.is_empty() {
        return None;
    }

    let path = Path::new(path);
    if !path.exists() {
        return None;
    }

    Some(
        path.canonicalize()
            .unwrap_or_else(|_| path.to_path_buf())
            .to_string_lossy()
            .to_string(),
    )
}

fn load_cached_color_image(path: &str) -> Option<egui::ColorImage> {
    let bytes = fs::read(path).ok()?;
    let image = ::image::load_from_memory(&bytes).ok()?.to_rgba8();
    let size = [image.width() as usize, image.height() as usize];
    Some(egui::ColorImage::from_rgba_unmultiplied(size, image.as_raw()))
}

fn fit_image_size(original: Vec2, max_size: Vec2) -> Vec2 {
    if original.x <= 0.0 || original.y <= 0.0 {
        return max_size;
    }

    let scale = (max_size.x / original.x)
        .min(max_size.y / original.y)
        .min(1.0);
    original * scale
}

pub fn get_nickname_lens(target_id: String, messages: &Vec<NapcatMessage>) -> (&str, Vec<usize>) {
    let mut nickname = "";
    let mut lens: Vec<usize> = vec![];
    for message in messages {
        let mut len: usize = 0;
        for chain in &message.data.message {
            match &chain.variant {
                NapcatMessageChainType::Source(_) => {},
                NapcatMessageChainType::Text { data } => {
                    len += data.text.len();
                },
                NapcatMessageChainType::Image { .. } => {
                    len += 12;
                },
                NapcatMessageChainType::Unsupported => {},
            };
        }

        if message.data.sender.user_id.to_string() == *target_id {
            nickname = &message.data.sender.nickname;
        }
        lens.push(len)
    }

    (nickname, lens)
}

fn target_default_display_name(target_id: &str, messages: Option<&Vec<NapcatMessage>>) -> String {
    messages
        .map(|messages| get_nickname_lens(target_id.to_owned(), messages).0)
        .filter(|nickname| !nickname.is_empty())
        .unwrap_or(target_id)
        .to_owned()
}

fn target_display_name(manager: &NapcatMessageManager, target_id: &str) -> String {
    if let Some(display_name) = manager
        .chat_targets
        .get(target_id)
        .map(|metadata| metadata.display_name.trim())
        .filter(|display_name| !display_name.is_empty() && *display_name != target_id)
        .map(str::to_owned)
    {
        return display_name;
    }

    if let Some(automatic_name) = manager
        .chat_targets
        .get(target_id)
        .map(|metadata| metadata.automatic_name.trim())
        .filter(|automatic_name| !automatic_name.is_empty())
        .map(str::to_owned)
    {
        return automatic_name;
    }

    target_default_display_name(
        target_id,
        manager.messages.get(target_id),
    )
}

fn chat_group_title(group_name: &str, group: &ChatGroup, manager: &NapcatMessageManager) -> String {
    let member_names = group
        .members
        .iter()
        .map(|member_id| target_display_name(manager, member_id))
        .collect::<Vec<_>>();

    if member_names.is_empty() {
        format!("讨论组: {group_name}")
    } else {
        format!(
            "讨论组: {}: {}",
            group_name,
            member_names.join(", ")
        )
    }
}

fn target_unread_count(manager: &NapcatMessageManager, target_id: &str) -> usize {
    let read_count = manager
        .read_message_counts
        .get(target_id)
        .copied()
        .unwrap_or_default();

    manager
        .messages
        .get(target_id)
        .map(|messages| {
            messages
                .iter()
                .skip(read_count)
                .filter(|message| message.data.user_id != message.data.self_id)
                .count()
        })
        .unwrap_or_default()
}

fn chat_group_unread_count(manager: &NapcatMessageManager, group: &ChatGroup) -> usize {
    group
        .members
        .iter()
        .map(|member_id| target_unread_count(manager, member_id))
        .sum()
}

fn group_chat_inner_size(member_count: usize, max_rect: Rect) -> Vec2 {
    let desired_height = if member_count == 0 {
        GROUP_CHAT_MIN_HEIGHT + GROUP_BROADCAST_INPUT_HEIGHT
    } else {
        member_count as f32 * GROUP_MEMBER_CHAT_SIZE.y
            + member_count.saturating_sub(1) as f32 * GROUP_CHAT_SEPARATOR_HEIGHT
            + GROUP_BROADCAST_INPUT_HEIGHT
    };

    egui::vec2(
        (GROUP_MEMBER_CHAT_SIZE.x + 48.0)
            .min(max_rect.width())
            .max(CHAT_WINDOW_MIN_SIZE.x),
        desired_height
            .min(GROUP_CHAT_MAX_HEIGHT)
            .min(max_rect.height())
            .max(GROUP_CHAT_MIN_HEIGHT),
    )
}

fn group_chat_max_size(max_rect: Rect) -> Vec2 {
    egui::vec2(
        GROUP_CHAT_MAX_WIDTH
            .min(max_rect.width())
            .max(CHAT_WINDOW_MIN_SIZE.x),
        GROUP_CHAT_MAX_HEIGHT
            .min(max_rect.height())
            .max(CHAT_WINDOW_MIN_SIZE.y),
    )
}

fn draw_drop_preview(ctx: &Context, rect: Rect) {
    let rect = rect.shrink(4.0);
    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        Id::new("chat_drop_preview"),
    ));
    let fill = egui::Color32::from_rgba_unmultiplied(60, 210, 120, 28);
    let stroke = Stroke::new(
        2.0,
        egui::Color32::from_rgb(70, 220, 130),
    );

    painter.rect_filled(rect, 4.0, fill);
    paint_dotted_line(
        &painter,
        rect.left_top(),
        rect.right_top(),
        stroke,
    );
    paint_dotted_line(
        &painter,
        rect.right_top(),
        rect.right_bottom(),
        stroke,
    );
    paint_dotted_line(
        &painter,
        rect.right_bottom(),
        rect.left_bottom(),
        stroke,
    );
    paint_dotted_line(
        &painter,
        rect.left_bottom(),
        rect.left_top(),
        stroke,
    );
}

fn paint_unread_badge(ctx: &Context, window_rect: Rect, unread_count: usize) {
    if unread_count == 0 {
        return;
    }

    let painter = ctx.layer_painter(egui::LayerId::new(
        egui::Order::Foreground,
        Id::new((
            "unread_badge",
            window_rect.min.x.to_bits(),
            window_rect.min.y.to_bits(),
        )),
    ));
    let badge_text = if unread_count > 99 { "99+".to_owned() } else { unread_count.to_string() };
    let radius = if unread_count > 99 { 11.0 } else { 9.0 };
    let center = Pos2::new(
        window_rect.right() - 18.0,
        window_rect.top() + 16.0,
    );

    painter.circle_filled(
        center,
        radius,
        egui::Color32::from_rgb(235, 55, 55),
    );
    painter.text(
        center,
        egui::Align2::CENTER_CENTER,
        badge_text,
        egui::FontId::proportional(10.0),
        egui::Color32::WHITE,
    );
}

fn paint_dotted_line(painter: &Painter, start: Pos2, end: Pos2, stroke: Stroke) {
    let line = end - start;
    let length = line.length();
    if length <= 0.0 {
        return;
    }

    let direction = line / length;
    let dash = 7.0;
    let gap = 5.0;
    let mut offset = 0.0;

    while offset < length {
        let segment_end = (offset + dash).min(length);
        painter.line_segment(
            [start + direction * offset, start + direction * segment_end],
            stroke,
        );
        offset += dash + gap;
    }
}

struct PlayerTextLine {
    player_message_count: usize,
    text: String,
    summary_eligible: bool,
}

#[derive(Debug, Clone, PartialEq, Eq)]
enum SummaryScope {
    Private,
    GroupPublic,
    GroupParty(String),
}

impl SummaryScope {
    fn summary_key(&self, campaign_id: &str, target_id: &str) -> String {
        let scope_key = match self {
            SummaryScope::Private => target_id.to_owned(),
            SummaryScope::GroupPublic => format!("group:{target_id}:public"),
            SummaryScope::GroupParty(party_id) => format!("group:{target_id}:party:{party_id}"),
        };
        format!(
            "campaign:{}:{scope_key}",
            encode_summary_key_component(campaign_id)
        )
    }

    fn label(&self) -> String {
        match self {
            SummaryScope::Private => "私聊".to_owned(),
            SummaryScope::GroupPublic => "公开".to_owned(),
            SummaryScope::GroupParty(party_id) => format!("小队：{party_id}"),
        }
    }
}

fn encode_summary_key_component(value: &str) -> String {
    value
        .as_bytes()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn decode_summary_key_component(value: &str) -> Option<String> {
    if !value.len().is_multiple_of(2) {
        return None;
    }
    let bytes = (0..value.len())
        .step_by(2)
        .map(|index| u8::from_str_radix(&value[index..index + 2], 16).ok())
        .collect::<Option<Vec<_>>>()?;
    String::from_utf8(bytes).ok()
}

fn parse_campaign_summary_key(summary_key: &str) -> Option<(String, &str)> {
    let rest = summary_key.strip_prefix("campaign:")?;
    let (encoded_campaign_id, scope_key) = rest.split_once(':')?;
    Some((
        decode_summary_key_component(encoded_campaign_id)?,
        scope_key,
    ))
}

fn player_text_lines(messages: &[CampaignMessage]) -> Vec<PlayerTextLine> {
    let mut player_message_count = 0;
    let mut lines = Vec::new();

    for message in messages {
        let text = message.text.trim();

        if text.is_empty() {
            continue;
        }

        player_message_count += 1;
        lines.push(PlayerTextLine {
            player_message_count,
            summary_eligible: !is_scene_capture_command_text(text),
            text: format!("{}: {}", message.sender_name, text),
        });
    }

    lines
}

fn queue_summaries_if_needed(
    manager: &NapcatMessageManager,
    target_id: &str,
    messages: &[NapcatMessage],
    summarized_message_counts: &HashMap<String, usize>,
    deepseek_sender: Option<&DeepseekIOSender>,
    deepseek_manager: &mut DeepseekManager,
) -> bool {
    let mut changed = false;
    let Some(campaign_id) = manager.active_campaign_id() else {
        return false;
    };
    for scope in summary_scopes_for_target(manager, target_id, messages) {
        let summary_key = scope.summary_key(&campaign_id, target_id);
        let summarized_message_count = summarized_message_counts
            .get(&summary_key)
            .copied()
            .unwrap_or_default();
        changed |= queue_summary_if_needed_for_scope(
            manager,
            target_id,
            messages,
            &scope,
            &summary_key,
            summarized_message_count,
            deepseek_sender,
            deepseek_manager,
        );
    }
    changed
}

fn queue_summary_if_needed_for_scope(
    manager: &NapcatMessageManager,
    target_id: &str,
    messages: &[NapcatMessage],
    scope: &SummaryScope,
    summary_key: &str,
    summarized_message_count: usize,
    deepseek_sender: Option<&DeepseekIOSender>,
    deepseek_manager: &mut DeepseekManager,
) -> bool {
    let campaign_messages =
        campaign_messages_for_summary_scope(manager, target_id, messages, scope);
    let lines = player_text_lines(&campaign_messages);
    let message_count = lines
        .last()
        .map(|line| line.player_message_count)
        .unwrap_or_default();
    if message_count == 0 || message_count < summarized_message_count + 5 {
        return false;
    }
    if summarized_message_count >= message_count {
        return false;
    }

    if let Some(summary) = deepseek_manager.summaries.get(summary_key) {
        if summary
            .blocks
            .iter()
            .any(|block| block.message_count == message_count)
        {
            return false;
        }
    }

    let Some(deepseek_sender) = deepseek_sender else {
        return false;
    };

    let text = lines
        .iter()
        .filter(|line| {
            line.player_message_count > summarized_message_count && line.summary_eligible
        })
        .map(|line| line.text.clone())
        .collect::<Vec<_>>()
        .join("\n");
    if text.is_empty() {
        return false;
    }

    let request = DeepseekRequest::Summary {
        target_id: summary_key.to_owned(),
        message_count,
        text,
        custom_prompt: String::new(),
    };

    let send_result = serde_json::to_string(&request)
        .map(|request| Message::Text(request.into()))
        .map_err(|err| err.to_string())
        .and_then(|request| {
            deepseek_sender
                .0
                .try_send(request)
                .map_err(|err| err.to_string())
        });

    match send_result {
        Ok(()) => {
            deepseek_manager
                .summaries
                .entry(summary_key.to_owned())
                .or_default()
                .upsert_block(DeepseekSummaryBlock {
                    latest: String::new(),
                    message_count,
                    pending: true,
                    error: None,
                });
            true
        },
        Err(error) => {
            deepseek_manager
                .summaries
                .entry(summary_key.to_owned())
                .or_default()
                .upsert_block(DeepseekSummaryBlock {
                    latest: String::new(),
                    message_count,
                    pending: false,
                    error: Some(error),
                });
            true
        },
    }
}

fn summary_scopes_for_target(
    manager: &NapcatMessageManager,
    target_id: &str,
    messages: &[NapcatMessage],
) -> Vec<SummaryScope> {
    let Some(campaign_id) = manager.active_campaign_id() else {
        return Vec::new();
    };
    if target_kind_with_messages(manager, target_id, messages) != ChatTargetExportKind::Group {
        return vec![SummaryScope::Private];
    }
    let mut party_ids = BTreeSet::new();
    if let Some(group) = manager.current_group().filter(|group| {
        group
            .group_chats
            .iter()
            .any(|group_id| group_id == target_id)
    }) {
        party_ids.extend(group.parties.keys().cloned());
    }

    for message in messages {
        if message.data.user_id == message.data.self_id {
            continue;
        }
        let campaign_message = manager.campaign_message_for_target(target_id, message);
        if campaign_message.campaign_id != campaign_id {
            continue;
        }
        if let Visibility::Party(party_id) = campaign_message.visibility {
            party_ids.insert(party_id);
        }
    }

    let mut scopes = vec![SummaryScope::GroupPublic];
    scopes.extend(party_ids.into_iter().map(SummaryScope::GroupParty));
    scopes
}

fn campaign_messages_for_summary_scope(
    manager: &NapcatMessageManager,
    target_id: &str,
    messages: &[NapcatMessage],
    scope: &SummaryScope,
) -> Vec<CampaignMessage> {
    let Some(campaign_id) = manager.active_campaign_id() else {
        return Vec::new();
    };
    match scope {
        SummaryScope::Private => manager.visible_campaign_messages_for_summary(target_id, messages),
        SummaryScope::GroupPublic => messages
            .iter()
            .filter(|message| message.data.user_id != message.data.self_id)
            .map(|message| manager.campaign_message_for_target(target_id, message))
            .filter(|message| message.campaign_id == campaign_id)
            .filter(|message| matches!(message.visibility, Visibility::Public))
            .collect(),
        SummaryScope::GroupParty(party_id) => messages
            .iter()
            .filter(|message| message.data.user_id != message.data.self_id)
            .map(|message| manager.campaign_message_for_target(target_id, message))
            .filter(|message| message.campaign_id == campaign_id)
            .filter(|message| {
                matches!(message.visibility, Visibility::Public)
                    || matches!(&message.visibility, Visibility::Party(message_party) if message_party == party_id)
            })
            .collect(),
    }
}

fn sync_summarized_message_counts(
    manager: &mut NapcatMessageManager,
    deepseek_manager: &DeepseekManager,
) -> bool {
    let mut changed = false;

    for (target_id, summary) in &deepseek_manager.summaries {
        let latest_summarized_count = summary
            .blocks
            .iter()
            .filter(|block| !block.pending && block.error.is_none())
            .map(|block| block.message_count)
            .max()
            .unwrap_or_default();

        if latest_summarized_count == 0 {
            continue;
        }

        let entry = manager
            .summarized_message_counts
            .entry(target_id.clone())
            .or_default();
        if *entry < latest_summarized_count {
            *entry = latest_summarized_count;
            changed = true;
        }
    }

    changed
}

fn summary_display_parts<'a>(
    manager: &NapcatMessageManager,
    summary_key: &'a str,
) -> (String, String) {
    let (campaign_id, scope_key) = parse_campaign_summary_key(summary_key)
        .map(|(campaign_id, scope_key)| (Some(campaign_id), scope_key))
        .unwrap_or((None, summary_key));
    let (target_id, mut scope) = parse_group_summary_key(scope_key)
        .map(|(target_id, scope)| (target_id, scope.label()))
        .unwrap_or_else(|| {
            let scope = if manager.chat_target_kind(scope_key) == ChatTargetExportKind::Group {
                "全部（旧）".to_owned()
            } else {
                "私聊".to_owned()
            };
            (scope_key, scope)
        });
    if let Some(campaign_id) = campaign_id {
        scope = format!("{scope} · 战役：{campaign_id}");
    }
    let display_name = manager
        .messages
        .get(target_id)
        .map(|messages| {
            get_nickname_lens(target_id.to_string(), messages)
                .0
                .to_owned()
        })
        .filter(|nickname| !nickname.is_empty())
        .unwrap_or_else(|| target_id.to_string());

    (display_name, scope)
}

fn parse_group_summary_key(summary_key: &str) -> Option<(&str, SummaryScope)> {
    let rest = summary_key.strip_prefix("group:")?;
    let mut parts = rest.splitn(3, ':');
    let target_id = parts.next()?;
    let scope_kind = parts.next()?;
    match scope_kind {
        "public" => Some((target_id, SummaryScope::GroupPublic)),
        "party" => Some((
            target_id,
            SummaryScope::GroupParty(parts.next()?.to_owned()),
        )),
        _ => None,
    }
}

fn summary_panel(ui: &mut Ui, manager: &NapcatMessageManager, deepseek_manager: &DeepseekManager) {
    ui.heading("DeepSeek 总结");
    ui.separator();

    if deepseek_manager.summaries.is_empty() {
        ui.label("暂无总结");
        ui.allocate_rect(
            ui.available_rect_before_wrap(),
            egui::Sense::hover(),
        );
        return;
    }

    egui::ScrollArea::vertical().show(ui, |ui| {
        let mut summaries = deepseek_manager.summaries.iter().collect::<Vec<_>>();
        summaries.sort_by_key(|(target_id, _)| target_id.as_str());

        for (target_id, summary) in summaries {
            let (nickname, scope) = summary_display_parts(manager, target_id);

            ui.group(|ui| {
                ui.label(format!(
                    "{} / {} / {} 个总结",
                    nickname,
                    scope,
                    summary.blocks.len()
                ));
                for block in &summary.blocks {
                    let start = block.message_count.saturating_sub(4);
                    ui.separator();
                    ui.label(format!(
                        "{}-{} 条",
                        start, block.message_count
                    ));
                    if block.pending {
                        ui.label("总结中...");
                    } else if let Some(error) = &block.error {
                        ui.colored_label(egui::Color32::LIGHT_RED, error);
                    } else {
                        ui.label(block.latest.trim());
                    }
                }
            });
        }
    });
}

fn pending_chat_requests_window(
    ctx: &Context,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    napcat_sender: Option<&NapcatIOSender>,
    ime: &mut ResMut<ImeManager>,
) {
    if manager.pending_chat_targets.is_empty() {
        return;
    }

    let mut pending_targets = manager
        .pending_chat_targets
        .iter()
        .cloned()
        .collect::<Vec<_>>();
    pending_targets.sort();

    let mut changed = false;
    egui::Window::new("新的聊天请求")
        .id(Id::new("pending_chat_requests_window"))
        .default_pos(Pos2::new(16.0, 48.0))
        .default_size(Vec2::new(300.0, 120.0))
        .resizable(false)
        .collapsible(false)
        .show(ctx, |ui| {
            ui.label("NapCat收到了还没有打开窗口的聊天消息。");
            ui.separator();

            for target_id in pending_targets {
                let display_name = target_display_name(manager, &target_id);
                ui.horizontal(|ui| {
                    ui.label(display_name);
                    ui.with_layout(
                        egui::Layout::right_to_left(egui::Align::Center),
                        |ui| {
                            if ui.button("创建聊天").clicked() {
                                if manager.approve_chat_target(&target_id) {
                                    changed = true;
                                    if let (Some(sender), Some(text), Some(target)) = (
                                        napcat_sender,
                                        approval_onboarding_text(manager, &target_id),
                                        private_broadcast_target(manager, &target_id),
                                    ) {
                                        if let Err(err) =
                                            ime.queue_text_send(&target_id, text, sender, vec![
                                                target,
                                            ])
                                        {
                                            eprintln!(
                                                "failed to queue NapCat onboarding message: {err}"
                                            );
                                        }
                                    }
                                }
                            }
                            if ui.button("拒绝").clicked() {
                                changed |= manager.reject_chat_target(&target_id);
                            }
                        },
                    );
                });
            }
        });

    if changed {
        manager.persist().ok();
    }
}

fn approval_onboarding_text(manager: &NapcatMessageManager, target_id: &str) -> Option<String> {
    if manager.chat_target_kind(target_id) != ChatTargetExportKind::Private {
        return None;
    }

    let group = manager.group_for_player_target(target_id)?;

    let guide = group.guide.trim();
    if guide.is_empty() {
        None
    } else {
        Some(format!("团内引导：\n{guide}"))
    }
}

fn waiting_turn_manager_window(
    ctx: &Context,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
) {
    let Some(group_name) = manager.current_trpg_group.clone() else {
        return;
    };
    let Some(group) = manager.trpg_groups.get(&group_name) else {
        return;
    };

    let waiting_players = group
        .players
        .iter()
        .filter(|target_id| {
            group
                .player_turns
                .get(*target_id)
                .map(|turn| !turn.acted && !turn.skipped)
                .unwrap_or(true)
        })
        .map(|target_id| {
            (
                target_id.clone(),
                target_display_name(manager, target_id),
            )
        })
        .collect::<Vec<_>>();

    let mut target_to_focus = None;
    egui::Window::new("轮次管理")
        .id(Id::new("waiting_turn_manager_window"))
        .default_pos(Pos2::new(240.0, 48.0))
        .default_size(Vec2::new(240.0, 220.0))
        .min_size(Vec2::new(180.0, 120.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(group_name.as_str());
                ui.small(format!(
                    "{}人等待中",
                    waiting_players.len()
                ));
            });
            ui.separator();

            if waiting_players.is_empty() {
                ui.label("所有玩家都已行动。");
                return;
            }

            egui::ScrollArea::vertical()
                .id_salt("waiting_turn_manager_players")
                .show(ui, |ui| {
                    for (target_id, display_name) in &waiting_players {
                        if ui.button(display_name).on_hover_text(target_id).clicked() {
                            target_to_focus = Some(target_id.clone());
                        }
                    }
                });
        });

    if let Some(target_id) = target_to_focus {
        let opened = manager.open_chat_targets.insert(target_id.clone());
        manager.pending_chat_targets.remove(&target_id);
        focus_standalone_chat_window(ctx, &target_id);
        if opened {
            manager.persist().ok();
        }
    }
}

fn chat_target_kind_label(manager: &NapcatMessageManager, target_id: &str) -> &'static str {
    match manager.chat_target_kind(target_id) {
        ChatTargetExportKind::Group => "群",
        ChatTargetExportKind::Private => "私聊",
        ChatTargetExportKind::Unknown => "聊天",
    }
}

fn target_kind_with_messages(
    manager: &NapcatMessageManager,
    target_id: &str,
    messages: &[NapcatMessage],
) -> ChatTargetExportKind {
    match messages.first().map(|message| &message.data.message_type) {
        Some(NapcatMessageType::Private) => ChatTargetExportKind::Private,
        Some(NapcatMessageType::Group) => ChatTargetExportKind::Group,
        None => manager.chat_target_kind(target_id),
    }
}

fn is_group_chat_target(manager: &NapcatMessageManager, target_id: &str) -> bool {
    manager.chat_target_kind(target_id) == ChatTargetExportKind::Group
}

fn sorted_pool_targets(manager: &NapcatMessageManager, group_chats: bool) -> Vec<String> {
    let mut targets = manager
        .chat_target_export_entries()
        .into_iter()
        .map(|entry| entry.target_id)
        .filter(|target_id| is_group_chat_target(manager, target_id) == group_chats)
        .collect::<Vec<_>>();
    targets.sort_by(|a, b| target_display_name(manager, a).cmp(&target_display_name(manager, b)));
    targets
}

fn trpg_group_member_count(group: &TrpgGroup) -> usize {
    group.players.len() + group.group_chats.len()
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct ChatListTargetView {
    target_id: String,
    message_count: usize,
    total_message_count: usize,
    unread_count: usize,
    last_time: u64,
}

fn chat_list_player_filter_options(manager: &NapcatMessageManager) -> Vec<String> {
    let mut players = manager
        .current_group()
        .map(|group| group.players.clone())
        .unwrap_or_default();

    if players.is_empty() {
        players = manager
            .chat_target_export_entries()
            .into_iter()
            .filter_map(|entry| {
                (entry.kind == ChatTargetExportKind::Private)
                    .then(|| entry.target_id.parse::<u64>().ok().map(|_| entry.target_id))
                    .flatten()
            })
            .collect();
    }

    players.retain(|player_id| player_id.parse::<u64>().is_ok());
    players.sort_by(|left, right| {
        target_display_name(manager, left)
            .cmp(&target_display_name(manager, right))
            .then_with(|| left.cmp(right))
    });
    players.dedup();
    players
}

fn chat_list_target_views(
    manager: &NapcatMessageManager,
    player_filter: Option<u64>,
) -> Vec<ChatListTargetView> {
    let mut targets = manager
        .chat_target_export_entries()
        .into_iter()
        .filter_map(|entry| {
            let messages = manager
                .messages
                .get(&entry.target_id)
                .map(Vec::as_slice)
                .unwrap_or_default();
            chat_list_target_view(
                manager,
                &entry.target_id,
                messages,
                player_filter,
            )
        })
        .collect::<Vec<_>>();
    targets.sort_by(|left, right| {
        right
            .last_time
            .cmp(&left.last_time)
            .then_with(|| left.target_id.cmp(&right.target_id))
    });
    targets
}

fn chat_list_target_view(
    manager: &NapcatMessageManager,
    target_id: &str,
    messages: &[NapcatMessage],
    player_filter: Option<u64>,
) -> Option<ChatListTargetView> {
    let (message_count, last_time, unread_count) = match player_filter {
        Some(player_id) => {
            let visible_messages =
                manager.visible_messages_for_player(target_id, messages, player_id);
            let message_count = visible_messages.len();
            if message_count == 0 {
                return None;
            }
            let last_time = visible_messages
                .last()
                .map(|message| message.data.time)
                .unwrap_or_default();
            let unread_count =
                target_unread_count_for_player(manager, target_id, messages, player_id);
            (message_count, last_time, unread_count)
        },
        None => {
            let last_time = messages
                .last()
                .map(|message| message.data.time)
                .unwrap_or_default();
            (
                messages.len(),
                last_time,
                target_unread_count(manager, target_id),
            )
        },
    };

    Some(ChatListTargetView {
        target_id: target_id.to_owned(),
        message_count,
        total_message_count: messages.len(),
        unread_count,
        last_time,
    })
}

fn target_unread_count_for_player(
    manager: &NapcatMessageManager,
    target_id: &str,
    messages: &[NapcatMessage],
    player_id: u64,
) -> usize {
    let read_count = manager
        .read_message_counts
        .get(target_id)
        .copied()
        .unwrap_or_default();
    let Some(campaign_id) = manager.active_campaign_id() else {
        return 0;
    };
    let access = manager.player_access_for_user(player_id);

    messages
        .iter()
        .skip(read_count)
        .filter(|message| message.data.user_id != message.data.self_id)
        .filter(|message| {
            let campaign_message = manager.campaign_message_for_target(target_id, message);
            campaign_message.campaign_id == campaign_id
                && access.can_read(&campaign_message.visibility)
        })
        .count()
}

fn chat_list_panel(
    ui: &mut Ui,
    ctx: &Context,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    edit_target: &mut Option<String>,
    edit_name: &mut String,
    player_visible_filter: &mut Option<String>,
    trpg_group_settings: &mut TrpgGroupSettingsState,
) {
    ui.heading("TRPG组");
    ui.add_space(4.0);

    let mut trpg_group_names = manager.trpg_groups.keys().cloned().collect::<Vec<_>>();
    trpg_group_names.sort();
    if trpg_group_names.is_empty() {
        ui.label("还没有TRPG组。");
    } else {
        for group_name in trpg_group_names {
            let Some(group) = manager.trpg_groups.get(&group_name).cloned() else {
                continue;
            };
            let unread_count = group
                .players
                .iter()
                .chain(group.group_chats.iter())
                .map(|target_id| target_unread_count(manager, target_id))
                .sum::<usize>();

            ui.group(|ui| {
                ui.set_width(ui.available_width());
                ui.horizontal(|ui| {
                    ui.label(&group_name);
                    if unread_count > 0 {
                        ui.label(format!("({unread_count})"));
                    }
                });
                ui.small(format!(
                    "{}名玩家，{}个群聊，第{}轮",
                    group.players.len(),
                    group.group_chats.len(),
                    group.world_turn
                ));
                if ui.button("打开工作区").clicked() {
                    trpg_group_settings.open = true;
                    trpg_group_settings.focused_group_name = Some(group_name.clone());
                }
            });
            ui.add_space(4.0);
        }
    }

    ui.separator();
    ui.heading("聊天");
    ui.add_space(4.0);

    if manager.messages.is_empty() {
        ui.label("还没有保存的聊天。");
        return;
    }

    let player_filter_options = chat_list_player_filter_options(manager);
    if player_filter_options.is_empty() {
        *player_visible_filter = None;
    } else {
        let mut filter_enabled = player_visible_filter.is_some();
        if ui
            .checkbox(
                &mut filter_enabled,
                "按玩家可见筛选列表",
            )
            .on_hover_text("只显示所选玩家可读取的聊天目标和消息计数")
            .changed()
        {
            if filter_enabled {
                *player_visible_filter = Some(player_filter_options[0].clone());
            } else {
                *player_visible_filter = None;
            }
        }

        if filter_enabled {
            let selected =
                player_visible_filter.get_or_insert_with(|| player_filter_options[0].clone());
            if !player_filter_options
                .iter()
                .any(|player_id| player_id == selected)
            {
                *selected = player_filter_options[0].clone();
            }

            egui::ComboBox::from_id_salt("chat_list_player_visible_filter")
                .selected_text(target_display_name(manager, selected))
                .show_ui(ui, |ui| {
                    for player_id in &player_filter_options {
                        ui.selectable_value(
                            selected,
                            player_id.clone(),
                            target_display_name(manager, player_id),
                        );
                    }
                });
        }
    }

    let player_filter_id = player_visible_filter
        .as_deref()
        .and_then(|player_id| player_id.parse::<u64>().ok());
    let targets = chat_list_target_views(manager, player_filter_id);
    if targets.is_empty() {
        ui.label("所选玩家当前没有可见聊天。");
        return;
    }

    let mut changed = false;
    egui::ScrollArea::vertical()
        .id_salt("chat_list_panel_scroll")
        .auto_shrink([false, false])
        .show(ui, |ui| {
            for target in targets {
                let target_id = target.target_id;
                let display_name = target_display_name(manager, &target_id);
                let unread_count = target.unread_count;
                let is_open = manager.open_chat_targets.contains(&target_id);
                let is_editing = edit_target.as_deref() == Some(target_id.as_str());

                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    ui.horizontal(|ui| {
                        if ui
                            .selectable_label(is_open, display_name)
                            .on_hover_text(&target_id)
                            .clicked()
                        {
                            manager.open_chat_targets.insert(target_id.clone());
                            manager.pending_chat_targets.remove(&target_id);
                            focus_standalone_chat_window(ctx, &target_id);
                            changed = true;
                        }

                        if unread_count > 0 {
                            ui.label(format!("({unread_count})"));
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.small(chat_target_kind_label(
                            manager, &target_id,
                        ));
                        ui.small(&target_id);
                        if player_filter_id.is_some() {
                            ui.small(format!(
                                "可见 {}/{}",
                                target.message_count, target.total_message_count
                            ));
                        }
                    });

                    if is_editing {
                        ui.text_edit_singleline(edit_name);
                        ui.horizontal(|ui| {
                            if ui.button("保存").clicked() {
                                manager
                                    .chat_targets
                                    .entry(target_id.clone())
                                    .or_default()
                                    .display_name = edit_name.trim().to_owned();
                                *edit_target = None;
                                edit_name.clear();
                                changed = true;
                            }
                            if ui.button("取消").clicked() {
                                *edit_target = None;
                                edit_name.clear();
                            }
                            if ui.button("清除").clicked() {
                                if let Some(metadata) = manager.chat_targets.get_mut(&target_id) {
                                    metadata.display_name.clear();
                                }
                                *edit_target = None;
                                edit_name.clear();
                                changed = true;
                            }
                        });
                    } else {
                        ui.horizontal(|ui| {
                            if ui.button("编辑").clicked() {
                                *edit_target = Some(target_id.clone());
                                *edit_name = manager
                                    .chat_targets
                                    .get(&target_id)
                                    .map(|metadata| metadata.display_name.clone())
                                    .filter(|name| !name.trim().is_empty())
                                    .unwrap_or_else(|| target_display_name(manager, &target_id));
                            }
                            let close_label = if is_open { "关闭" } else { "打开" };
                            if ui.button(close_label).clicked() {
                                if is_open {
                                    manager.open_chat_targets.remove(&target_id);
                                } else {
                                    manager.open_chat_targets.insert(target_id.clone());
                                    manager.pending_chat_targets.remove(&target_id);
                                    focus_standalone_chat_window(ctx, &target_id);
                                }
                                changed = true;
                            }
                        });
                    }
                });
                ui.add_space(4.0);
            }
        });

    if changed {
        manager.persist().ok();
    }
}

fn set_target_membership(targets: &mut Vec<String>, target_id: &str, selected: bool) {
    if selected {
        if !targets.iter().any(|existing| existing == target_id) {
            targets.push(target_id.to_owned());
        }
    } else {
        targets.retain(|existing| existing != target_id);
    }
}

fn character_status_summary_ui(ui: &mut Ui, character: &PlayerCharacter) {
    let display_name = if character.nickname.trim().is_empty() {
        character.name.trim()
    } else {
        character.nickname.trim()
    };
    let display_name = if display_name.is_empty() { "未命名角色" } else { display_name };
    let state_label = if character.inited {
        "已完成"
    } else {
        character_creation_step_label(character.creation_step)
    };

    ui.horizontal_wrapped(|ui| {
        ui.strong(display_name);
        ui.small(state_label);
        ui.small(format!(
            "HP {}/{} [{}]",
            format_character_number(character.hp),
            format_character_number(character.max_hp),
            character_hp_status(character.hp, character.max_hp)
        ));
        ui.small(format!(
            "MP {}/{}",
            format_character_number(character.mp),
            format_character_number(character.max_mp)
        ));
        ui.small(format!("Lv {}", character.level));
        ui.small(format!(
            "速度 {}",
            format_character_number(character.speed)
        ));
    });

    egui::Grid::new(ui.next_auto_id())
        .num_columns(4)
        .spacing([10.0, 2.0])
        .show(ui, |ui| {
            status_summary_value_ui(
                ui,
                "STR",
                character.status.str_,
                character.extra_status.str_,
            );
            status_summary_value_ui(
                ui,
                "AGI",
                character.status.agi,
                character.extra_status.agi,
            );
            status_summary_value_ui(
                ui,
                "DEX",
                character.status.dex,
                character.extra_status.dex,
            );
            status_summary_value_ui(
                ui,
                "VIT",
                character.status.vit,
                character.extra_status.vit,
            );
            ui.end_row();
            status_summary_value_ui(
                ui,
                "INT",
                character.status.int_,
                character.extra_status.int_,
            );
            status_summary_value_ui(
                ui,
                "WIS",
                character.status.wis,
                character.extra_status.wis,
            );
            status_summary_value_ui(
                ui,
                "K",
                character.status.k,
                character.extra_status.k,
            );
            status_summary_value_ui(
                ui,
                "CHA",
                character.status.cha,
                character.extra_status.cha,
            );
            ui.end_row();
        });
}

#[derive(Clone)]
struct QuickCastSkill {
    index: usize,
    name: String,
    note: String,
    skill_type: Option<String>,
    legacy_buff_machine_json: Option<String>,
    mp_cost: f32,
    cooldown_turns: u32,
    cooldown_left: Option<u32>,
    target_count: Option<u32>,
    target_class: Option<String>,
    range: Option<i32>,
    arg_values: SkillRuleArgs,
}

#[derive(Clone)]
enum QuickCastEffect {
    Damage {
        amount: f32,
        target: TargetSelector,
        damage_type: DamageType,
    },
    Heal {
        amount: f32,
        target: TargetSelector,
    },
    GrantBuff {
        target: TargetSelector,
        buff: crate::rule_engine::RuleBuffTemplate,
    },
    Sequence(Vec<QuickCastResolvedEffect>),
}

#[derive(Clone)]
struct QuickCastResolvedEffect {
    effect: QuickCastEffect,
    targets: Vec<String>,
}

struct QuickCastAction {
    caster_id: String,
    skill: QuickCastSkill,
    targets: Vec<String>,
    effect: Option<QuickCastEffect>,
    cast_turn: u32,
    force: bool,
}

fn quick_character_windows(
    ctx: &Context,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    quick_character_targets: &mut Local<HashSet<String>>,
    character_edit_state: &mut CharacterEditState,
    rule_engine_state: &mut RuleEngineState,
    scene_positions: Option<&SceneCharacterPositions>,
    player_camera_positions: Option<&ScenePlayerCameraPositions>,
) {
    let mut target_ids = quick_character_targets.iter().cloned().collect::<Vec<_>>();
    target_ids.sort();

    let mut closed_targets = Vec::new();
    for target_id in target_ids {
        if is_group_chat_target(manager, &target_id) {
            closed_targets.push(target_id);
            continue;
        }

        let display_name = target_display_name(manager, &target_id);
        let character_targets = quick_cast_character_targets(manager);
        let cast_turn = quick_cast_cooldown_turn(manager, &target_id);
        let mut open = true;
        let mut changed = false;
        let mut cast_action = None;
        let window_max_width = ctx
            .content_rect()
            .width()
            .min(CHARACTER_WINDOW_MAX_WIDTH)
            .max(CHARACTER_WINDOW_MIN_WIDTH);
        let window_max_height = (ctx.content_rect().height() - 32.0).max(240.0);
        egui::Window::new(format!("角色：{display_name}"))
            .id(Id::new((
                "quick_character_window",
                target_id.as_str(),
            )))
            .open(&mut open)
            .default_width(CHARACTER_WINDOW_DEFAULT_WIDTH)
            .min_width(CHARACTER_WINDOW_MIN_WIDTH)
            .max_width(window_max_width)
            .max_height(window_max_height)
            .vscroll(true)
            .resizable(true)
            .show(ctx, |ui| {
                ui.set_max_width(window_max_width);
                ui.horizontal(|ui| {
                    ui.small("玩家");
                    ui.monospace(&target_id);
                });
                let skill_pool_snapshot = manager.skill_pool.clone();
                let item_pool_snapshot = manager.item_pool.clone();
                let stat_config = manager.character_stat_config_for_target(&target_id);
                let character = manager
                    .player_characters
                    .entry(target_id.clone())
                    .or_default();
                if character_effect_sync_needed(&target_id, character) {
                    sync_character_buffs(
                        &target_id,
                        character,
                        &stat_config,
                        rule_engine_state,
                        &skill_pool_snapshot,
                    );
                    changed = true;
                }
                character_status_summary_ui(ui, character);
                ui.separator();
                cast_action = quick_cast_ui(
                    ui,
                    &target_id,
                    character,
                    character_edit_state,
                    &character_targets,
                    &skill_pool_snapshot,
                    cast_turn,
                    scene_positions,
                    player_camera_positions,
                );
                ui.separator();
                ui.collapsing("编辑角色", |ui| {
                    changed |= character_editor_ui(
                        ui,
                        &target_id,
                        character,
                        &display_name,
                        character_edit_state,
                        rule_engine_state,
                        &skill_pool_snapshot,
                        &item_pool_snapshot,
                        stat_config,
                    );
                });
            });

        if !open {
            closed_targets.push(target_id);
        }
        if let Some(action) = cast_action {
            changed |= apply_quick_cast_action(manager, rule_engine_state, action);
        }
        if changed {
            manager.persist().ok();
        }
    }

    for target_id in closed_targets {
        quick_character_targets.remove(&target_id);
    }
}

fn quick_cast_ui(
    ui: &mut Ui,
    caster_id: &str,
    character: &mut PlayerCharacter,
    edit_state: &mut CharacterEditState,
    character_targets: &[(String, String)],
    skill_pool: &[SkillPoolEntry],
    cast_turn: u32,
    scene_positions: Option<&SceneCharacterPositions>,
    player_camera_positions: Option<&ScenePlayerCameraPositions>,
) -> Option<QuickCastAction> {
    crate::napcat::materialize_imported_skill_cooldowns(character, cast_turn);
    let skills = quick_cast_skills(character);
    if skills.is_empty() {
        ui.small("没有可释放技能。");
        return None;
    }

    let selected = edit_state
        .quick_cast_skill_index
        .entry(caster_id.to_owned())
        .or_insert(0);
    if *selected >= skills.len() {
        *selected = 0;
    }
    let skill = skills[*selected].clone();
    let mut effect = quick_cast_effect(
        &skill.note,
        &skill.arg_values,
        skill.skill_type.as_deref(),
        skill.legacy_buff_machine_json.as_deref(),
        skill_pool,
    );
    let cooldown_remaining = quick_skill_cooldown_remaining(
        character,
        skill.index,
        skill.cooldown_turns,
        skill.cooldown_left,
        cast_turn,
    );
    let targets = effect
        .as_mut()
        .map(|effect| {
            resolve_quick_cast_effect_targets(
                caster_id,
                character,
                effect,
                character_targets,
                scene_positions,
                player_camera_positions,
                &skill,
            )
        })
        .unwrap_or_default();
    let can_pay = character.mp + f32::EPSILON >= skill.mp_cost;
    let can_cast = can_pay && cooldown_remaining == 0 && effect.is_some();
    let force_pending = edit_state
        .pending_force_cast
        .as_ref()
        .is_some_and(|(target_id, index)| target_id == caster_id && *index == *selected);

    let mut action = None;
    egui::CollapsingHeader::new("释放技能")
        .default_open(true)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("技能");
                egui::ComboBox::from_id_salt(format!("quick_cast_skill_{caster_id}"))
                    .selected_text(skill.name.as_str())
                    .show_ui(ui, |ui| {
                        for (index, skill) in skills.iter().enumerate() {
                            let remaining = quick_skill_cooldown_remaining(
                                character,
                                skill.index,
                                skill.cooldown_turns,
                                skill.cooldown_left,
                                cast_turn,
                            );
                            let mut details = Vec::new();
                            if skill.mp_cost > 0.0 {
                                details.push(format!(
                                    "MP {}",
                                    format_character_number(skill.mp_cost)
                                ));
                            }
                            if remaining > 0 {
                                details.push(format!("CD {remaining}"));
                            } else if skill.cooldown_turns > 0 {
                                details.push(format!("CD {}", skill.cooldown_turns));
                            }
                            if let Some(target_count) = skill.target_count {
                                details.push(format!("目标 {target_count}"));
                            }
                            if let Some(target_class) = skill
                                .target_class
                                .as_deref()
                                .map(str::trim)
                                .filter(|target_class| !target_class.is_empty())
                            {
                                details.push(target_class.to_owned());
                            }
                            if let Some(radius) = skill_range_radius(skill.range) {
                                details.push(format!(
                                    "范围 {}",
                                    format_character_number(radius)
                                ));
                            }
                            let label = if details.is_empty() {
                                skill.name.clone()
                            } else {
                                format!(
                                    "{} ({})",
                                    skill.name,
                                    details.join(", ")
                                )
                            };
                            ui.selectable_value(selected, index, label);
                        }
                    });
                if let Some(radius) = effect.as_ref().and_then(|effect| {
                    quick_cast_radius(
                        effect,
                        quick_cast_skill_range_radius(
                            character,
                            effect,
                            skill.range,
                            skill.skill_type.as_deref(),
                        ),
                    )
                }) {
                    ui.small(format!(
                        "以玩家镜头为中心 {}米",
                        format_character_number(radius)
                    ));
                }
            });

            if let Some(effect) = effect.as_ref() {
                let target_label = match effect {
                    QuickCastEffect::Damage { .. } => "范围内目标",
                    QuickCastEffect::Heal { .. } => "可影响角色",
                    QuickCastEffect::GrantBuff { .. } => "可获得状态",
                    QuickCastEffect::Sequence(_) => "可影响角色",
                };
                if targets.is_empty() {
                    ui.small("范围内没有可影响角色。");
                } else {
                    ui.horizontal_wrapped(|ui| {
                        ui.small(target_label);
                        for target_id in &targets {
                            let name = character_targets
                                .iter()
                                .find(|(candidate_id, _)| candidate_id == target_id)
                                .map(|(_, name)| name.as_str())
                                .unwrap_or(target_id.as_str());
                            ui.small(name);
                        }
                    });
                }
            } else {
                ui.small("技能描述需要是可解析的固定伤害或治疗规则。");
            }

            let response = ui.add_enabled(can_cast, egui::Button::new("释放"));
            if response.clicked() {
                action = Some(QuickCastAction {
                    caster_id: caster_id.to_owned(),
                    skill: skill.clone(),
                    targets: targets.clone(),
                    effect: effect.clone(),
                    cast_turn,
                    force: false,
                });
                edit_state.pending_force_cast = None;
            }

            ui.horizontal_wrapped(|ui| {
                let force_response = ui
                    .add(egui::Button::new("强制释放"))
                    .on_hover_text("GM强制释放：忽略MP、目标和规则解析条件。");
                if force_response.clicked() {
                    if force_pending {
                        action = Some(QuickCastAction {
                            caster_id: caster_id.to_owned(),
                            skill: skill.clone(),
                            targets: targets.clone(),
                            effect: effect.clone(),
                            cast_turn,
                            force: true,
                        });
                        edit_state.pending_force_cast = None;
                    } else {
                        edit_state.pending_force_cast = Some((caster_id.to_owned(), *selected));
                    }
                }
                if force_pending {
                    ui.small("再次点击强制释放确认。");
                    if ui.small_button("取消").clicked() {
                        edit_state.pending_force_cast = None;
                    }
                }
            });

            if cooldown_remaining > 0 {
                ui.small(format!(
                    "冷却还剩{cooldown_remaining}轮"
                ));
            }
            if !can_cast && can_pay && cooldown_remaining == 0 {
                ui.small("普通释放需要可解析的固定伤害或治疗规则；强制释放可忽略。");
            }
            if !can_pay {
                ui.small(format!(
                    "需要{} MP",
                    format_character_number(skill.mp_cost)
                ));
            }
        });
    action
}

fn quick_cast_skills(character: &mut PlayerCharacter) -> Vec<QuickCastSkill> {
    normalize_character_skill_fields(character);
    character
        .skill_names
        .iter()
        .enumerate()
        .filter_map(|(index, name)| {
            let metadata = character.skill_metadata[index].clone();
            if !metadata.is_approved() {
                return None;
            }
            let name = if name.trim().is_empty() {
                format!("技能{}", index + 1)
            } else {
                name.trim().to_owned()
            };
            Some(QuickCastSkill {
                index,
                name,
                note: character
                    .skill_notes
                    .get(index)
                    .cloned()
                    .unwrap_or_default(),
                skill_type: metadata.skill_type.clone(),
                legacy_buff_machine_json: metadata.legacy_buff_machine_json,
                mp_cost: character_effective_skill_mp_cost(
                    character,
                    character
                        .skill_mp_costs
                        .get(index)
                        .copied()
                        .unwrap_or_default(),
                    metadata.skill_type.as_deref(),
                ),
                cooldown_turns: character
                    .skill_cooldown_turns
                    .get(index)
                    .copied()
                    .unwrap_or_default(),
                cooldown_left: metadata.cooldown_left,
                target_count: metadata.target_count,
                target_class: metadata.target_class,
                range: metadata.range,
                arg_values: skill_rule_args(&metadata.args),
            })
        })
        .collect()
}

fn quick_cast_cooldown_turn(manager: &NapcatMessageManager, caster_id: &str) -> u32 {
    manager
        .trpg_groups
        .values()
        .filter(|group| group.players.iter().any(|player_id| player_id == caster_id))
        .map(|group| {
            group
                .player_turns
                .get(caster_id)
                .map(|turn| turn.turns_passed)
                .unwrap_or(group.world_turn)
        })
        .max()
        .unwrap_or_default()
}

fn quick_skill_cooldown_remaining(
    character: &PlayerCharacter,
    skill_index: usize,
    cooldown_turns: u32,
    cooldown_left: Option<u32>,
    cast_turn: u32,
) -> u32 {
    let skill_key = skill_index.to_string();
    if let Some(last_cast_turn) = character.skill_last_cast_turns.get(&skill_key) {
        return cooldown_turns.saturating_sub(cast_turn.saturating_sub(*last_cast_turn));
    }
    character
        .skill_cooldown_ready_turns
        .get(&skill_key)
        .map(|ready_turn| ready_turn.saturating_sub(cast_turn))
        .unwrap_or_else(|| cooldown_left.unwrap_or_default())
}

fn quick_cast_effect(
    note: &str,
    arg_values: &SkillRuleArgs,
    skill_type: Option<&str>,
    legacy_buff_machine_json: Option<&str>,
    skill_pool: &[SkillPoolEntry],
) -> Option<QuickCastEffect> {
    let legacy_pool_entries = legacy_moonberry_pool_entries(skill_pool);
    let ast = parse_rule_with_named_args(
        note,
        &arg_values.numeric_values,
        &arg_values.text_values,
    )
    .ok()
    .map(|ast| apply_skill_type_damage_default(ast, skill_type))
    .or_else(|| {
        legacy_buff_machine_json.and_then(|json| {
            legacy_moonberry_buff_machine_skill_cast_rule_with_context(
                json,
                &arg_values.numeric_values,
                &arg_values.text_values,
                skill_type,
                &legacy_pool_entries,
            )
        })
    })?;
    let mut effects = ast
        .actions
        .into_iter()
        .filter_map(|action| match action {
            Action::Damage {
                target,
                amount: ValueExpr::Number(amount),
                damage_type,
            } => Some(QuickCastEffect::Damage {
                amount: amount.max(0.0),
                target,
                damage_type,
            }),
            Action::Heal {
                target,
                amount: ValueExpr::Number(amount),
            } => Some(QuickCastEffect::Heal {
                amount: amount.max(0.0),
                target,
            }),
            Action::GrantBuff { target, buff } => Some(QuickCastEffect::GrantBuff { target, buff }),
            _ => None,
        })
        .collect::<Vec<_>>();
    match effects.len() {
        0 => None,
        1 => effects.pop(),
        _ => Some(QuickCastEffect::Sequence(
            effects
                .into_iter()
                .map(|effect| QuickCastResolvedEffect {
                    effect,
                    targets: Vec::new(),
                })
                .collect(),
        )),
    }
}

fn quick_cast_skill_range_radius(
    character: &PlayerCharacter,
    effect: &QuickCastEffect,
    range: Option<i32>,
    skill_type: Option<&str>,
) -> Option<f32> {
    let minimum_range = match effect {
        QuickCastEffect::Damage {
            damage_type: DamageType::Range,
            ..
        } => character_minimum_range_meters(character),
        QuickCastEffect::Sequence(effects) => {
            return effects
                .iter()
                .filter_map(|resolved| {
                    quick_cast_skill_range_radius(
                        character,
                        &resolved.effect,
                        range,
                        skill_type,
                    )
                })
                .max_by(f32::total_cmp);
        },
        _ => 0.0,
    };
    let range_multiplier = if moonberry_skill_type_is_spell(skill_type) {
        character_spell_range_multiplier(character)
    } else {
        1.0
    };
    moonberry_effective_skill_range_radius_with_multiplier(range, minimum_range, range_multiplier)
}

fn quick_cast_radius(effect: &QuickCastEffect, fallback_radius: Option<f32>) -> Option<f32> {
    if let QuickCastEffect::Sequence(effects) = effect {
        return effects
            .iter()
            .filter_map(|resolved| quick_cast_radius(&resolved.effect, fallback_radius))
            .max_by(f32::total_cmp);
    }
    let target = match effect {
        QuickCastEffect::Damage { target, .. }
        | QuickCastEffect::Heal { target, .. }
        | QuickCastEffect::GrantBuff { target, .. } => target,
        QuickCastEffect::Sequence(_) => unreachable!("sequence handled above"),
    };
    target
        .area
        .and_then(|area| area.radius_meters.or(fallback_radius))
}

fn skill_range_radius(range: Option<i32>) -> Option<f32> {
    range.filter(|range| *range > 0).map(|range| range as f32)
}

fn trpg_damage_bonus_kind(damage_type: DamageType) -> TrpgDamageBonusKind {
    match damage_type {
        DamageType::Magical => TrpgDamageBonusKind::Magical,
        DamageType::Physical => TrpgDamageBonusKind::Physical,
        DamageType::Range => TrpgDamageBonusKind::Range,
        DamageType::Cursed
        | DamageType::Diseased
        | DamageType::Bleed
        | DamageType::Poisoning
        | DamageType::None => TrpgDamageBonusKind::Other,
    }
}

fn trpg_damage_taken_kind(damage_type: DamageType) -> TrpgDamageTakenKind {
    match damage_type {
        DamageType::Magical => TrpgDamageTakenKind::Magical,
        DamageType::Diseased => TrpgDamageTakenKind::Diseased,
        DamageType::Poisoning => TrpgDamageTakenKind::Poisoning,
        DamageType::Physical
        | DamageType::Range
        | DamageType::Cursed
        | DamageType::Bleed
        | DamageType::None => TrpgDamageTakenKind::Other,
    }
}

fn resolve_quick_cast_effect_targets(
    caster_id: &str,
    character: &PlayerCharacter,
    effect: &mut QuickCastEffect,
    character_targets: &[(String, String)],
    scene_positions: Option<&SceneCharacterPositions>,
    player_camera_positions: Option<&ScenePlayerCameraPositions>,
    skill: &QuickCastSkill,
) -> Vec<String> {
    if let QuickCastEffect::Sequence(effects) = effect {
        let mut targets = Vec::new();
        let mut seen = HashSet::new();
        for resolved in effects {
            resolved.targets = resolve_quick_cast_effect_targets(
                caster_id,
                character,
                &mut resolved.effect,
                character_targets,
                scene_positions,
                player_camera_positions,
                skill,
            );
            for target_id in &resolved.targets {
                if seen.insert(target_id.clone()) {
                    targets.push(target_id.clone());
                }
            }
        }
        return targets;
    }

    let fallback_radius = quick_cast_skill_range_radius(
        character,
        effect,
        skill.range,
        skill.skill_type.as_deref(),
    );
    limit_skill_targets(
        quick_cast_targets(
            caster_id,
            effect,
            character_targets,
            scene_positions,
            player_camera_positions,
            fallback_radius,
            skill.target_class.as_deref(),
        ),
        skill_target_limit(
            skill.target_count,
            skill.target_class.as_deref(),
        ),
    )
}

fn quick_cast_targets(
    caster_id: &str,
    effect: &QuickCastEffect,
    character_targets: &[(String, String)],
    scene_positions: Option<&SceneCharacterPositions>,
    player_camera_positions: Option<&ScenePlayerCameraPositions>,
    fallback_radius: Option<f32>,
    target_class: Option<&str>,
) -> Vec<String> {
    let target = match effect {
        QuickCastEffect::Damage { target, .. }
        | QuickCastEffect::Heal { target, .. }
        | QuickCastEffect::GrantBuff { target, .. } => target,
        QuickCastEffect::Sequence(effects) => {
            let mut targets = Vec::new();
            let mut seen = HashSet::new();
            for target_id in effects.iter().flat_map(|resolved| &resolved.targets) {
                if seen.insert(target_id.clone()) {
                    targets.push(target_id.clone());
                }
            }
            return targets;
        },
    };
    let force_area =
        skill_target_class_is_area(target_class) && !matches!(target.actor, ActorRef::SelfActor);
    if target.area.is_some() || force_area {
        let radius = target
            .area
            .and_then(|area| area.radius_meters)
            .or(fallback_radius);
        let Some(radius) = radius else {
            return character_targets
                .iter()
                .filter(|(target_id, _)| target_id != caster_id)
                .map(|(target_id, _)| target_id.clone())
                .collect();
        };
        let Some(user_id) = caster_id.parse::<u64>().ok() else {
            return Vec::new();
        };
        let Some(camera_position) =
            player_camera_positions.and_then(|positions| positions.positions.get(&user_id))
        else {
            return Vec::new();
        };
        let Some(scene_positions) = scene_positions else {
            return Vec::new();
        };
        return character_targets
            .iter()
            .filter(|(target_id, _)| target_id != caster_id)
            .filter(|(target_id, _)| {
                scene_positions
                    .positions
                    .get(target_id)
                    .map(|position| camera_position.distance(*position) <= radius)
                    .unwrap_or(false)
            })
            .map(|(target_id, _)| target_id.clone())
            .collect();
    }

    match target.actor {
        ActorRef::SelfActor => vec![caster_id.to_owned()],
        ActorRef::Source | ActorRef::Target => {
            let targets = character_targets
                .iter()
                .find(|(target_id, _)| target_id != caster_id)
                .map(|(target_id, _)| vec![target_id.clone()])
                .unwrap_or_default();
            filter_quick_cast_targets_by_range(
                caster_id,
                targets,
                scene_positions,
                player_camera_positions,
                fallback_radius,
            )
        },
    }
}

fn filter_quick_cast_targets_by_range(
    caster_id: &str,
    targets: Vec<String>,
    scene_positions: Option<&SceneCharacterPositions>,
    player_camera_positions: Option<&ScenePlayerCameraPositions>,
    radius: Option<f32>,
) -> Vec<String> {
    let Some(radius) = radius else {
        return targets;
    };
    let Some(user_id) = caster_id.parse::<u64>().ok() else {
        return Vec::new();
    };
    let Some(camera_position) =
        player_camera_positions.and_then(|positions| positions.positions.get(&user_id))
    else {
        return Vec::new();
    };
    let Some(scene_positions) = scene_positions else {
        return Vec::new();
    };
    targets
        .into_iter()
        .filter(|target_id| {
            scene_positions
                .positions
                .get(target_id)
                .map(|position| camera_position.distance(*position) <= radius)
                .unwrap_or(false)
        })
        .collect()
}

fn limit_skill_targets(mut targets: Vec<String>, target_count: Option<u32>) -> Vec<String> {
    if let Some(target_count) = target_count {
        targets.truncate(target_count as usize);
    }
    targets
}

fn skill_target_limit(target_count: Option<u32>, target_class: Option<&str>) -> Option<u32> {
    match target_class.map(str::trim) {
        Some("无目标") => Some(0),
        Some("单目标") => Some(target_count.unwrap_or(1).min(1)),
        _ => target_count,
    }
}

fn skill_target_class_is_area(target_class: Option<&str>) -> bool {
    matches!(
        target_class.map(str::trim),
        Some("范围")
    )
}

fn quick_cast_character_targets(manager: &NapcatMessageManager) -> Vec<(String, String)> {
    let mut targets = manager
        .player_characters
        .iter()
        .filter(|(_, character)| character.inited && character.hp > 0.0)
        .map(|(target_id, character)| {
            let display_name = if !character.nickname.trim().is_empty() {
                character.nickname.trim().to_owned()
            } else if !character.name.trim().is_empty() {
                character.name.trim().to_owned()
            } else {
                target_display_name(manager, target_id)
            };
            (target_id.clone(), display_name)
        })
        .collect::<Vec<_>>();
    targets.sort_by(|left, right| left.1.cmp(&right.1).then_with(|| left.0.cmp(&right.0)));
    targets
}

fn apply_quick_cast_action(
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    rule_engine_state: &mut RuleEngineState,
    action: QuickCastAction,
) -> bool {
    let mut affected_ids = action.targets.clone();
    if !affected_ids
        .iter()
        .any(|target_id| target_id == &action.caster_id)
    {
        affected_ids.push(action.caster_id.clone());
    }
    let changed = apply_quick_cast_action_to_manager(manager.as_mut(), action);
    if changed {
        let skill_pool = manager.skill_pool.clone();
        for target_id in affected_ids {
            let stat_config = manager.character_stat_config_for_target(&target_id);
            let Some(character) = manager.player_characters.get_mut(&target_id) else {
                continue;
            };
            sync_character_buffs(
                &target_id,
                character,
                &stat_config,
                rule_engine_state,
                &skill_pool,
            );
        }
    }
    changed
}

fn apply_quick_cast_action_to_manager(
    manager: &mut NapcatMessageManager,
    mut action: QuickCastAction,
) -> bool {
    let sequence = match action.effect.take() {
        Some(QuickCastEffect::Sequence(effects)) => Some(effects),
        effect => {
            action.effect = effect;
            None
        },
    };
    if let Some(effects) = sequence {
        let mut changed = false;
        for (index, resolved) in effects.into_iter().enumerate() {
            let mut skill = action.skill.clone();
            if index > 0 {
                skill.mp_cost = 0.0;
                skill.cooldown_turns = 0;
                skill.cooldown_left = None;
            }
            let applied = apply_quick_cast_action_to_manager(manager, QuickCastAction {
                caster_id: action.caster_id.clone(),
                skill,
                targets: resolved.targets,
                effect: Some(resolved.effect),
                cast_turn: action.cast_turn,
                force: action.force || index > 0,
            });
            if index == 0 && !applied {
                return false;
            }
            changed |= applied;
        }
        return changed;
    }
    let stat_config = manager.character_stat_config_for_target(&action.caster_id);
    let effect = action.effect;
    let (
        source_damage_multiplier,
        source_healing_multiplier,
        source_physical_damage_lifesteal,
        source_physical_damage_followup_rate,
        source_minimum_damage_floor,
        source_mutual_aid_healing_rate,
        source_dying_target_healing_modifier,
        damage_dealt_buffs,
    ) = {
        let Some(caster) = manager.player_characters.get_mut(&action.caster_id) else {
            return false;
        };
        crate::napcat::materialize_imported_skill_cooldowns(caster, action.cast_turn);
        if !action.force && caster.mp + f32::EPSILON < action.skill.mp_cost {
            return false;
        }
        let cooldown_remaining = quick_skill_cooldown_remaining(
            caster,
            action.skill.index,
            action.skill.cooldown_turns,
            action.skill.cooldown_left,
            action.cast_turn,
        );
        if !action.force && cooldown_remaining > 0 {
            return false;
        }
        let source_damage_multiplier = match effect {
            Some(QuickCastEffect::Damage { damage_type, .. }) => {
                caster.damage_dealt_modifier
                    * character_low_hp_damage_multiplier(caster)
                    * character_damage_attribute_multiplier(
                        caster,
                        &stat_config,
                        trpg_damage_bonus_kind(damage_type),
                    )
                    * moonberry_chaos_output_multiplier(character_chaos_output_variance(caster))
            },
            _ => caster.damage_dealt_modifier,
        };
        let source_healing_multiplier = caster.healing_dealt_modifier
            * character_healing_attribute_multiplier(caster, &stat_config)
            * wounded_healing_dealt_multiplier(
                caster.hp,
                caster.max_hp,
                character_wounded_healing_dealt_modifier(caster),
            );
        let source_healing_multiplier = match effect {
            Some(QuickCastEffect::Heal { .. }) => {
                source_healing_multiplier
                    * moonberry_chaos_output_multiplier(character_chaos_output_variance(caster))
            },
            _ => source_healing_multiplier,
        };
        let source_physical_damage_lifesteal = match effect {
            Some(QuickCastEffect::Damage {
                damage_type: DamageType::Physical,
                ..
            }) => character_physical_damage_lifesteal(caster),
            _ => 0.0,
        };
        let source_physical_damage_followup_rate = match effect {
            Some(QuickCastEffect::Damage {
                damage_type: DamageType::Physical,
                ..
            }) => character_physical_damage_followup_rate(caster),
            _ => 0.0,
        };
        let source_minimum_damage_floor = match effect {
            Some(QuickCastEffect::Damage { .. }) => character_minimum_damage_floor(caster),
            _ => 0.0,
        };
        let source_mutual_aid_healing_rate = match effect {
            Some(QuickCastEffect::Heal { .. }) => character_mutual_aid_healing_rate(caster),
            _ => 0.0,
        };
        let source_dying_target_healing_modifier = character_dying_target_healing_modifier(caster);
        let damage_dealt_buffs = character_damage_dealt_talent_buffs(caster, &action.caster_id);
        if !action.force {
            caster.mp = (caster.mp - action.skill.mp_cost).max(0.0);
        }
        caster.skill_last_cast_turns.insert(
            action.skill.index.to_string(),
            action.cast_turn,
        );
        caster
            .skill_cooldown_ready_turns
            .remove(&action.skill.index.to_string());
        (
            source_damage_multiplier,
            source_healing_multiplier,
            source_physical_damage_lifesteal,
            source_physical_damage_followup_rate,
            source_minimum_damage_floor,
            source_mutual_aid_healing_rate,
            source_dying_target_healing_modifier,
            damage_dealt_buffs,
        )
    };

    let mut changed = true;
    let Some(effect) = effect else {
        return changed;
    };
    let mut pending_source_lifesteal = 0.0;
    let mut pending_source_mutual_aid_healing = 0.0;
    for target_id in limit_skill_targets(
        action.targets,
        skill_target_limit(
            action.skill.target_count,
            action.skill.target_class.as_deref(),
        ),
    ) {
        let Some(target) = manager.player_characters.get_mut(&target_id) else {
            continue;
        };
        match effect {
            QuickCastEffect::Damage {
                amount,
                damage_type,
                ..
            } => {
                let target_damage_multiplier = target.damage_taken_modifier
                    * character_damage_taken_attribute_multiplier(
                        target,
                        trpg_damage_taken_kind(damage_type),
                    );
                let incoming_amount =
                    (amount * source_damage_multiplier * target_damage_multiplier).max(0.0);
                let typed_final_amount = (incoming_amount
                    * large_hit_damage_taken_multiplier(
                        target.max_hp,
                        incoming_amount,
                        character_large_hit_damage_taken_modifier(target),
                    ))
                .max(0.0);
                let final_amount =
                    if amount > f32::EPSILON && source_minimum_damage_floor > f32::EPSILON {
                        typed_final_amount.max(source_minimum_damage_floor)
                    } else {
                        typed_final_amount
                    };
                let (damage_changed, effective_amount) =
                    apply_effective_character_damage(target, final_amount);
                changed |= damage_changed;
                if effective_amount > f32::EPSILON {
                    for buff in damage_dealt_buffs.iter().cloned() {
                        if upsert_character_active_buff(target, buff) {
                            changed = true;
                        }
                    }
                    if damage_type == DamageType::Physical {
                        pending_source_lifesteal +=
                            effective_amount * source_physical_damage_lifesteal;
                        if source_physical_damage_followup_rate > f32::EPSILON {
                            target
                                .active_buffs
                                .push(moonberry_physical_damage_followup_buff(
                                    &action.caster_id,
                                    effective_amount * source_physical_damage_followup_rate,
                                ));
                            changed = true;
                        }
                    }
                }
            },
            QuickCastEffect::Heal { amount, .. } => {
                let target_healing_multiplier = target.healing_taken_modifier
                    * dying_target_healing_multiplier(
                        target.hp,
                        target.max_hp,
                        source_dying_target_healing_modifier,
                    );
                let final_amount =
                    (amount * source_healing_multiplier * target_healing_multiplier).max(0.0);
                let target_mutual_aid_healing_rate = character_mutual_aid_healing_rate(target);
                let (healing_changed, effective_amount) =
                    apply_effective_character_healing(target, final_amount);
                changed |= healing_changed;
                if target_id != action.caster_id && effective_amount > f32::EPSILON {
                    pending_source_mutual_aid_healing += effective_amount
                        * (source_mutual_aid_healing_rate + target_mutual_aid_healing_rate);
                }
            },
            QuickCastEffect::GrantBuff { ref buff, .. } => {
                target
                    .active_buffs
                    .push(buff.to_buff_spec(&action.caster_id));
                changed = true;
            },
            QuickCastEffect::Sequence(_) => unreachable!("sequence expanded before resolution"),
        }
    }
    let pending_source_healing = pending_source_lifesteal + pending_source_mutual_aid_healing;
    if pending_source_healing > f32::EPSILON {
        if let Some(caster) = manager.player_characters.get_mut(&action.caster_id) {
            changed |= apply_effective_character_healing(caster, pending_source_healing).0;
        }
    }
    changed
}

fn apply_effective_character_healing(character: &mut PlayerCharacter, amount: f32) -> (bool, f32) {
    let previous_hp = character.hp;
    let next_hp = (character.hp + amount.max(0.0)).min(character.max_hp);
    let effective_amount = (next_hp - previous_hp).max(0.0);
    if effective_amount <= f32::EPSILON {
        return (false, 0.0);
    }
    character.hp = next_hp;
    if let Some(base_stats) = character.buff_base_stats.as_mut() {
        base_stats.hp = (base_stats.hp + effective_amount).min(base_stats.max_hp);
    }
    record_character_healing_taken(character, effective_amount);
    (true, effective_amount)
}

fn apply_effective_character_damage(character: &mut PlayerCharacter, amount: f32) -> (bool, f32) {
    let previous_hp = character.hp;
    let next_hp = (character.hp - amount.max(0.0)).max(0.0);
    let effective_amount = (previous_hp - next_hp).max(0.0);
    if effective_amount <= f32::EPSILON {
        return (false, 0.0);
    }
    character.hp = next_hp;
    if let Some(base_stats) = character.buff_base_stats.as_mut() {
        base_stats.hp = (base_stats.hp - effective_amount).max(0.0);
    }
    record_character_damage_taken(character, effective_amount);
    (true, effective_amount)
}

fn status_summary_value_ui(ui: &mut Ui, label: &str, base: i32, extra: i32) {
    let total = base + extra;
    if extra == 0 {
        ui.label(format!("{label} {base}"));
    } else {
        ui.label(format!(
            "{label} {base}+{extra}={total}"
        ));
    }
}

fn format_character_number(value: f32) -> String {
    if value.fract().abs() < f32::EPSILON {
        format!("{}", value as i32)
    } else {
        format!("{value:.1}")
    }
}

fn character_editor_ui(
    ui: &mut Ui,
    target_id: &str,
    character: &mut PlayerCharacter,
    chat_display_name: &str,
    edit_state: &mut CharacterEditState,
    rule_engine_state: &mut RuleEngineState,
    skill_pool: &[SkillPoolEntry],
    item_pool: &[InventoryItem],
    stat_config: TrpgBasicConfig,
) -> bool {
    let mut changed = false;
    let mut derived_stats_changed = false;
    ui.horizontal(|ui| {
        changed |= ui.checkbox(&mut character.inited, "已完成").changed();
        egui::ComboBox::from_label("流程")
            .selected_text(character_creation_step_label(
                character.creation_step,
            ))
            .show_ui(ui, |ui| {
                for (step, label) in character_creation_step_options() {
                    changed |= ui
                        .selectable_value(
                            &mut character.creation_step,
                            step,
                            label,
                        )
                        .changed();
                }
            });
        if ui.button("使用聊天名").clicked() {
            character.nickname = chat_display_name.to_owned();
            changed = true;
        }
        if ui.button("重置").clicked() {
            edit_state.pending_character_reset = Some(target_id.to_owned());
        }
    });

    if edit_state.pending_character_reset.as_deref() == Some(target_id) {
        let character_label = if character.name.trim().is_empty() {
            chat_display_name.to_owned()
        } else {
            character.name.trim().to_owned()
        };
        let modal = Modal::new(Id::new((
            "character_reset_confirm",
            target_id,
        )))
        .show(ui.ctx(), |ui| {
            ui.set_width(300.0);
            ui.heading("重置角色？");
            ui.label(format!(
                "这会清空{character_label}的所有角色数据。"
            ));
            ui.label("此操作无法撤销。");

            egui::Sides::new().show(
                ui,
                |ui| {
                    if ui.button("取消").clicked() {
                        ui.close();
                    }
                },
                |ui| {
                    if ui.button("重置").clicked() {
                        *character = PlayerCharacter::default();
                        edit_state.unlocked_status_targets.remove(target_id);
                        edit_state.gm_status_drafts.remove(target_id);
                        edit_state.buff_drafts.remove(target_id);
                        edit_state.pending_character_reset = None;
                        changed = true;
                    }
                },
            );
        });
        if modal.should_close() {
            edit_state.pending_character_reset = None;
        }
    }

    ui.columns(2, |columns| {
        columns[0].label("角色名");
        changed |= columns[0]
            .text_edit_singleline(&mut character.name)
            .changed();
        columns[1].label("昵称");
        changed |= columns[1]
            .text_edit_singleline(&mut character.nickname)
            .changed();
    });
    ui.label("图片URL");
    changed |= ui.text_edit_singleline(&mut character.image).changed();

    ui.separator();
    let status_unlocked = edit_state.unlocked_status_targets.contains(target_id);
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "创建点数剩余 {}",
            character.status_points
        ));
        ui.label(format!(
            "兑换点数 {}",
            character.exchange_points
        ));
        ui.label(format!(
            "HP状态：{}",
            character_hp_status(character.hp, character.max_hp)
        ));
        if status_unlocked {
            if ui.button("锁定").clicked() {
                edit_state.unlocked_status_targets.remove(target_id);
                edit_state.gm_status_drafts.remove(target_id);
            }
        } else if ui.button("解锁").clicked() {
            edit_state
                .unlocked_status_targets
                .insert(target_id.to_owned());
            edit_state.gm_status_drafts.insert(
                target_id.to_owned(),
                character.extra_status.clone(),
            );
        }
        let level_response = ui
            .add(
                egui::DragValue::new(&mut character.level)
                    .range(1..=999)
                    .prefix("等级 "),
            )
            .changed();
        changed |= level_response;
        derived_stats_changed |= level_response;
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.exp)
                    .range(0..=999_999)
                    .prefix("经验 "),
            )
            .changed();
        ui.label(format!(
            "/ {}",
            character_next_level_exp(character.level)
        ));
        let award_draft = edit_state
            .exp_award_drafts
            .entry(target_id.to_owned())
            .or_insert(0);
        ui.add(
            egui::DragValue::new(award_draft)
                .range(0..=999_999)
                .prefix("授予 "),
        );
        if ui.button("应用经验").clicked() && *award_draft > 0 {
            let level_ups = grant_character_experience(character, *award_draft);
            *award_draft = 0;
            changed = true;
            if level_ups > 0 {
                derived_stats_changed = true;
            }
        }
    });
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.hp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("HP "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.max_hp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("/ "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.hp_regen)
                    .range(-9999.0..=9999.0)
                    .speed(0.1)
                    .prefix("回复 "),
            )
            .changed();
    });
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.mp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("MP "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.max_mp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("/ "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.mp_regen)
                    .range(-9999.0..=9999.0)
                    .speed(0.1)
                    .prefix("回复 "),
            )
            .changed();
    });
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.speed)
                    .range(0.0..=9999.0)
                    .speed(0.1)
                    .prefix("速度 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.damage_dealt_modifier)
                    .range(0.0..=99.0)
                    .speed(0.01)
                    .prefix("伤害 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.damage_taken_modifier)
                    .range(0.0..=99.0)
                    .speed(0.01)
                    .prefix("承伤 "),
            )
            .changed();
    });
    ui.horizontal(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.healing_dealt_modifier)
                    .range(0.0..=99.0)
                    .speed(0.01)
                    .prefix("治疗 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.healing_taken_modifier)
                    .range(0.0..=99.0)
                    .speed(0.01)
                    .prefix("受疗 "),
            )
            .changed();
    });
    ui.horizontal_wrapped(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.damage_taken_this_turn)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("本轮承伤 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.healing_taken_this_turn)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("本轮受疗 "),
            )
            .changed();
        if ui.button("清空本轮").clicked() {
            changed |= reset_character_turn_totals(character);
        }
    });

    ui.separator();
    let status_changed = character_status_source_ui(
        ui,
        target_id,
        character,
        edit_state,
        status_unlocked,
    );
    changed |= status_changed;
    derived_stats_changed |= status_changed;
    ui.separator();
    changed |= character_buff_editor_ui(
        ui,
        target_id,
        character,
        edit_state,
        &stat_config,
        rule_engine_state,
        skill_pool,
    );
    ui.separator();
    let (inventory_changed, equipment_changed) = character_inventory_editor_ui(
        ui, target_id, character, edit_state, skill_pool, item_pool, false,
    );
    changed |= inventory_changed;
    if equipment_changed {
        sync_character_buffs(
            target_id,
            character,
            &stat_config,
            rule_engine_state,
            skill_pool,
        );
    }
    ui.separator();
    changed |= character_skill_editor_ui(
        ui,
        target_id,
        character,
        edit_state,
        rule_engine_state,
        skill_pool,
        stat_config,
    );

    if derived_stats_changed {
        if let Some(base_stats) = character.buff_base_stats.take() {
            restore_character_base_stats(character, base_stats);
        }
        update_character_from_status_with_config(character, &stat_config);
        sync_character_buffs(
            target_id,
            character,
            &stat_config,
            rule_engine_state,
            skill_pool,
        );
        changed = true;
    }

    if character.max_hp < 0.0 {
        character.max_hp = 0.0;
        changed = true;
    }
    if character.hp > character.max_hp {
        character.hp = character.max_hp;
        changed = true;
    }
    if character.mp > character.max_mp {
        character.mp = character.max_mp;
        changed = true;
    }

    changed
}

fn character_skill_pool_picker_ui(
    ui: &mut Ui,
    target_id: &str,
    character: &mut PlayerCharacter,
    edit_state: &mut CharacterEditState,
    skill_pool: &[SkillPoolEntry],
) -> bool {
    if skill_pool.is_empty() {
        return false;
    }

    let selected = edit_state
        .skill_pool_selected_index
        .entry(target_id.to_owned())
        .or_insert(0);
    if *selected >= skill_pool.len() {
        *selected = 0;
    }

    let mut changed = false;
    ui.collapsing("技能池", |ui| {
        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_id_salt(format!("skill_pool_picker_{target_id}"))
                .selected_text(skill_pool_entry_label(
                    &skill_pool[*selected],
                ))
                .show_ui(ui, |ui| {
                    for (index, entry) in skill_pool.iter().enumerate() {
                        ui.selectable_value(
                            selected,
                            index,
                            skill_pool_entry_label(entry),
                        );
                    }
                });
            if ui.button("复制到角色").clicked() {
                add_skill_pool_entry_to_character(character, &skill_pool[*selected]);
                changed = true;
            }
        });
        let entry = &skill_pool[*selected];
        ui.horizontal_wrapped(|ui| {
            ui.small(format!(
                "MP {}",
                format_character_number(entry.mp_cost)
            ));
            ui.small(format!(
                "冷却 {}轮",
                entry.cooldown_turns
            ));
            if let Some(source) = entry.source_character_name.as_deref() {
                ui.small(format!("来源 {source}"));
            } else {
                ui.small("来源 手动");
            }
            if let Some(category) = entry
                .category
                .as_deref()
                .filter(|category| !category.trim().is_empty())
            {
                ui.small(format!("类型 {category}"));
            }
            if !entry.tags.is_empty() {
                ui.small(format!("标签 {}", entry.tags.join(" ")));
            }
        });
        if let Some(legacy_label) = skill_pool_entry_legacy_label(entry) {
            ui.small(legacy_label);
        }
        if !entry.note.trim().is_empty() {
            ui.monospace(entry.note.trim());
        }
    });
    changed
}

fn add_skill_pool_entry_to_character(character: &mut PlayerCharacter, entry: &SkillPoolEntry) {
    normalize_character_skill_fields(character);
    character.skill_names.push(entry.name.clone());
    character.skill_notes.push(entry.note.clone());
    character.skill_mp_costs.push(entry.mp_cost.max(0.0));
    character.skill_cooldown_turns.push(entry.cooldown_turns);
    character
        .skill_metadata
        .push(CharacterSkillMetadata::skill_pool(
            entry,
        ));
}

fn skill_pool_entry_label(entry: &SkillPoolEntry) -> String {
    match entry.source_character_name.as_deref() {
        Some(source) if !source.trim().is_empty() => {
            format!(
                "{} - {}",
                skill_pool_entry_name(entry),
                source
            )
        },
        _ => skill_pool_entry_name(entry),
    }
}

fn character_skill_ui_id(target_id: &str, index: usize) -> egui::Id {
    egui::Id::new(("character_skill", target_id, index))
}

fn character_skill_editor_ui(
    ui: &mut Ui,
    target_id: &str,
    character: &mut PlayerCharacter,
    edit_state: &mut CharacterEditState,
    rule_engine_state: &mut RuleEngineState,
    skill_pool: &[SkillPoolEntry],
    stat_config: TrpgBasicConfig,
) -> bool {
    let mut changed = false;
    let mut remove_index = None;

    changed |= normalize_character_skill_fields(character);

    changed |= character_skill_pool_picker_ui(
        ui, target_id, character, edit_state, skill_pool,
    );

    ui.horizontal(|ui| {
        ui.label(format!(
            "技能描述：{}",
            character.skill_notes.len()
        ));
        if ui.button("+").on_hover_text("添加技能描述").clicked() {
            character.skill_names.push(String::new());
            character.skill_notes.push(String::new());
            character.skill_mp_costs.push(0.0);
            character.skill_cooldown_turns.push(0);
            character
                .skill_metadata
                .push(CharacterSkillMetadata::default());
            changed = true;
        }
    });

    for index in 0..character.skill_names.len() {
        let metadata = character.skill_metadata.get(index);
        let arg_values = metadata
            .map(|metadata| skill_rule_args(&metadata.args))
            .unwrap_or_default();
        let validation = parse_skill_note(
            &character.skill_notes[index],
            &arg_values,
            metadata.and_then(|metadata| metadata.skill_type.as_deref()),
        );
        ui.push_id(
            character_skill_ui_id(target_id, index),
            |ui| {
                ui.horizontal(|ui| {
                    let width =
                        (ui.available_width() - 28.0).clamp(160.0, CHARACTER_FIELD_MAX_WIDTH);
                    ui.vertical(|ui| {
                        ui.horizontal(|ui| {
                            ui.label("技能名");
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut character.skill_names[index])
                                        .desired_width((width - 78.0).max(82.0)),
                                )
                                .changed();
                        });
                        ui.horizontal_wrapped(|ui| {
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut character.skill_mp_costs[index])
                                        .range(0.0..=9999.0)
                                        .speed(1.0)
                                        .prefix("MP "),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::DragValue::new(
                                        &mut character.skill_cooldown_turns[index],
                                    )
                                    .range(0..=999)
                                    .speed(1)
                                    .prefix("冷却 "),
                                )
                                .changed();
                        });
                        ui.horizontal_wrapped(|ui| {
                            let metadata = &mut character.skill_metadata[index];
                            changed |= ui.checkbox(&mut metadata.pc_approved, "PC确认").changed();
                            changed |= ui.checkbox(&mut metadata.st_approved, "GM确认").changed();
                            if metadata.pc_approved && !metadata.st_approved {
                                ui.small("待GM确认");
                            }
                            if let Some(source) = character_skill_metadata_source_label(metadata) {
                                ui.small(source);
                            }
                        });
                        let metadata = &mut character.skill_metadata[index];
                        ui.collapsing("技能结构", |ui| {
                            changed |= character_skill_shape_metadata_ui(ui, metadata);
                        });
                        let response = ui.add(
                            egui::TextEdit::multiline(&mut character.skill_notes[index])
                                .desired_rows(2)
                                .desired_width(width),
                        );
                        if response.changed() {
                            changed = true;
                        }
                        if validation.is_err() {
                            let y = response.rect.bottom() - 2.0;
                            ui.painter().line_segment(
                                [
                                    egui::pos2(response.rect.left(), y),
                                    egui::pos2(response.rect.right(), y),
                                ],
                                Stroke::new(1.5, egui::Color32::RED),
                            );
                        }
                    });
                    if ui.button("-").on_hover_text("移除技能描述").clicked() {
                        remove_index = Some(index);
                    }
                });
                if let Err(err) = &validation {
                    ui.colored_label(egui::Color32::RED, err);
                }
            },
        );
    }

    if let Some(index) = remove_index {
        character.skill_names.remove(index);
        character.skill_notes.remove(index);
        character.skill_mp_costs.remove(index);
        character.skill_cooldown_turns.remove(index);
        character.skill_metadata.remove(index);
        shift_character_hotbar_after_remove(
            &mut character.inventory.hotbar,
            CharacterHotbarSlot::Skill(index),
        );
        shift_skill_last_cast_turns_after_remove(
            &mut character.skill_last_cast_turns,
            index,
        );
        changed = true;
    }

    sync_character_buffs(
        target_id,
        character,
        &stat_config,
        rule_engine_state,
        skill_pool,
    );

    changed
}

fn character_skill_shape_metadata_ui(ui: &mut Ui, metadata: &mut CharacterSkillMetadata) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        changed |= optional_known_string_field(
            ui,
            "类型",
            &mut metadata.skill_type,
            MOONBERRY_SKILL_TYPES,
            86.0,
        );
        changed |= optional_known_string_field(
            ui,
            "目标",
            &mut metadata.target_class,
            MOONBERRY_TARGET_CLASSES,
            86.0,
        );
        changed |= optional_u32_drag(
            ui,
            "数量",
            &mut metadata.target_count,
            0..=999,
        );
        changed |= optional_i32_drag(
            ui,
            "范围",
            &mut metadata.range,
            0..=9999,
        );
        changed |= optional_i32_drag(
            ui,
            "兑换点",
            &mut metadata.exchange_point,
            0..=9999,
        );
        changed |= optional_u32_drag(
            ui,
            "剩余冷却",
            &mut metadata.cooldown_left,
            0..=999,
        );
        changed |= optional_string_field(
            ui,
            "释放者",
            &mut metadata.legacy_caster,
            86.0,
        );
    });
    if !metadata.args.is_empty() {
        ui.small(format!(
            "旧变量：{}",
            metadata
                .args
                .iter()
                .map(skill_arg_label)
                .collect::<Vec<_>>()
                .join("，")
        ));
    }
    if let Some(trigger) = metadata
        .talent_trigger
        .as_deref()
        .filter(|trigger| !trigger.trim().is_empty())
    {
        ui.small(format!("天赋触发：{}", trigger.trim()));
    }
    if let Some(effect) = metadata
        .talent_effect
        .as_deref()
        .filter(|effect| !effect.trim().is_empty())
    {
        ui.small(format!("天赋效果：{}", effect.trim()));
    }
    if metadata.legacy_has_buff_machine || metadata.legacy_buff_machine_json.is_some() {
        let detail = metadata
            .legacy_buff_machine_json
            .as_deref()
            .map(|json| format!("，原始数据 {}字节", json.len()))
            .unwrap_or_default();
        ui.small(format!(
            "含旧buff机{detail}，常见主动/被动效果可执行。"
        ));
    }
    changed
}

fn optional_string_field(ui: &mut Ui, label: &str, value: &mut Option<String>, width: f32) -> bool {
    ui.label(label);
    let mut text = value.clone().unwrap_or_default();
    let changed = ui
        .add(egui::TextEdit::singleline(&mut text).desired_width(width))
        .changed();
    if changed {
        let text = text.trim();
        *value = (!text.is_empty()).then(|| text.to_owned());
    }
    changed
}

fn optional_known_string_field(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<String>,
    known_values: &[&str],
    width: f32,
) -> bool {
    let mut changed = false;
    ui.label(label);

    let selected_text = value
        .as_deref()
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .unwrap_or("未设置");
    let mut selected_value = value.clone().unwrap_or_default();
    egui::ComboBox::from_id_salt(ui.next_auto_id())
        .selected_text(selected_text)
        .show_ui(ui, |ui| {
            changed |= ui
                .selectable_value(
                    &mut selected_value,
                    String::new(),
                    "未设置",
                )
                .changed();
            for candidate in known_values {
                changed |= ui
                    .selectable_value(
                        &mut selected_value,
                        (*candidate).to_owned(),
                        *candidate,
                    )
                    .changed();
            }
        });

    if changed {
        let trimmed = selected_value.trim();
        *value = (!trimmed.is_empty()).then(|| trimmed.to_owned());
    }

    let mut text = value.clone().unwrap_or_default();
    let text_changed = ui
        .add(egui::TextEdit::singleline(&mut text).desired_width(width))
        .changed();
    if text_changed {
        let text = text.trim();
        *value = (!text.is_empty()).then(|| text.to_owned());
    }

    changed || text_changed
}

fn optional_u32_drag(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<u32>,
    range: std::ops::RangeInclusive<u32>,
) -> bool {
    let mut changed = false;
    let mut enabled = value.is_some();
    changed |= ui.checkbox(&mut enabled, label).changed();
    if enabled {
        let value_ref = value.get_or_insert(0);
        changed |= ui
            .add(egui::DragValue::new(value_ref).range(range).speed(1))
            .changed();
    } else if value.take().is_some() {
        changed = true;
    }
    changed
}

fn optional_i32_drag(
    ui: &mut Ui,
    label: &str,
    value: &mut Option<i32>,
    range: std::ops::RangeInclusive<i32>,
) -> bool {
    let mut changed = false;
    let mut enabled = value.is_some();
    changed |= ui.checkbox(&mut enabled, label).changed();
    if enabled {
        let value_ref = value.get_or_insert(0);
        changed |= ui
            .add(egui::DragValue::new(value_ref).range(range).speed(1))
            .changed();
    } else if value.take().is_some() {
        changed = true;
    }
    changed
}

fn skill_arg_label(arg: &crate::napcat::SkillPoolArg) -> String {
    let name = if arg.name.trim().is_empty() { "未命名变量" } else { arg.name.trim() };
    if arg.kind.trim().is_empty() && arg.value.trim().is_empty() {
        name.to_owned()
    } else if arg.value.trim().is_empty() {
        format!("{name}:{}", arg.kind.trim())
    } else if arg.kind.trim().is_empty() {
        format!("{name}={}", arg.value.trim())
    } else {
        format!(
            "{name}:{}={}",
            arg.kind.trim(),
            arg.value.trim()
        )
    }
}

fn character_skill_slot_count(character: &PlayerCharacter) -> usize {
    character
        .skill_names
        .len()
        .max(character.skill_notes.len())
        .max(character.skill_mp_costs.len())
        .max(character.skill_cooldown_turns.len())
        .max(character.skill_metadata.len())
}

fn pending_gm_skill_count(character: &PlayerCharacter) -> usize {
    (0..character_skill_slot_count(character))
        .filter(|index| {
            let metadata = character
                .skill_metadata
                .get(*index)
                .cloned()
                .unwrap_or_default();
            metadata.pc_approved && !metadata.st_approved
        })
        .count()
}

fn normalize_character_skill_fields(character: &mut PlayerCharacter) -> bool {
    let mut changed = false;
    let skill_count = character_skill_slot_count(character);
    if character.skill_names.len() != skill_count {
        character.skill_names.resize(skill_count, String::new());
        changed = true;
    }
    if character.skill_notes.len() != skill_count {
        character.skill_notes.resize(skill_count, String::new());
        changed = true;
    }
    if character.skill_mp_costs.len() != skill_count {
        character.skill_mp_costs.resize(skill_count, 0.0);
        changed = true;
    }
    if character.skill_cooldown_turns.len() != skill_count {
        character.skill_cooldown_turns.resize(skill_count, 0);
        changed = true;
    }
    if character.skill_metadata.len() != skill_count {
        character.skill_metadata.resize(
            skill_count,
            CharacterSkillMetadata::default(),
        );
        changed = true;
    }
    for cost in &mut character.skill_mp_costs {
        if *cost < 0.0 {
            *cost = 0.0;
            changed = true;
        }
    }
    if retain_valid_skill_last_cast_turns(
        &mut character.skill_last_cast_turns,
        skill_count,
    ) {
        changed = true;
    }
    changed
}

fn character_skill_metadata_source_label(metadata: &CharacterSkillMetadata) -> Option<String> {
    match metadata.source {
        CharacterSkillSourceKind::Manual => None,
        CharacterSkillSourceKind::Talent => {
            let label = metadata
                .source_pool_label
                .as_deref()
                .filter(|label| !label.trim().is_empty())
                .unwrap_or("天赋");
            Some(format!("来源：{label}"))
        },
        CharacterSkillSourceKind::SkillPool => {
            let label = metadata
                .source_pool_label
                .as_deref()
                .filter(|label| !label.trim().is_empty())
                .unwrap_or("技能池");
            Some(format!("来源：{label}"))
        },
    }
}

fn retain_valid_skill_last_cast_turns(
    last_cast_turns: &mut HashMap<String, u32>,
    skill_count: usize,
) -> bool {
    let before_len = last_cast_turns.len();
    last_cast_turns.retain(|key, _| {
        key.parse::<usize>()
            .ok()
            .is_some_and(|index| index < skill_count)
    });
    before_len != last_cast_turns.len()
}

fn shift_skill_last_cast_turns_after_remove(
    last_cast_turns: &mut HashMap<String, u32>,
    removed_index: usize,
) {
    let shifted = last_cast_turns
        .iter()
        .filter_map(|(key, turn)| {
            let index = key.parse::<usize>().ok()?;
            if index == removed_index {
                None
            } else if index > removed_index {
                Some(((index - 1).to_string(), *turn))
            } else {
                Some((key.clone(), *turn))
            }
        })
        .collect();
    *last_cast_turns = shifted;
}

fn parse_skill_note(
    note: &str,
    arg_values: &SkillRuleArgs,
    skill_type: Option<&str>,
) -> Result<Option<RuleAst>, String> {
    if note.trim().is_empty() {
        return Ok(None);
    }
    parse_rule_with_named_args(
        note,
        &arg_values.numeric_values,
        &arg_values.text_values,
    )
    .map(|ast| apply_skill_type_damage_default(ast, skill_type))
    .map(Some)
}

fn sync_character_skill_rules(
    target_id: &str,
    character: &PlayerCharacter,
    stat_config: &TrpgBasicConfig,
    rule_engine_state: &mut RuleEngineState,
    skill_pool: &[SkillPoolEntry],
) {
    let stats = CharacterBuffBaseStats::from_character(character);
    sync_character_skill_rules_with_stats(
        target_id,
        character,
        &stats,
        stat_config,
        rule_engine_state,
        skill_pool,
    );
}

fn sync_character_skill_rules_with_stats(
    target_id: &str,
    character: &PlayerCharacter,
    stats: &CharacterBuffBaseStats,
    stat_config: &TrpgBasicConfig,
    rule_engine_state: &mut RuleEngineState,
    skill_pool: &[SkillPoolEntry],
) {
    let legacy_pool_entries = legacy_moonberry_pool_entries(skill_pool);
    let rules = character
        .skill_notes
        .iter()
        .enumerate()
        .filter(|(index, _)| {
            character
                .skill_metadata
                .get(*index)
                .cloned()
                .unwrap_or_default()
                .is_approved()
        })
        .filter_map(|(index, note)| {
            let metadata = character.skill_metadata.get(index);
            let arg_values = metadata
                .map(|metadata| skill_rule_args(&metadata.args))
                .unwrap_or_default();
            let mut rules = Vec::new();
            if let Some(rule) = parse_skill_note(
                note,
                &arg_values,
                metadata.and_then(|metadata| metadata.skill_type.as_deref()),
            )
            .ok()
            .flatten()
            {
                rules.push(rule);
            }
            if let Some(rule) = metadata
                .and_then(|metadata| metadata.legacy_buff_machine_json.as_deref())
                .and_then(|json| {
                    legacy_moonberry_buff_machine_skill_cast_rule_with_context(
                        json,
                        &arg_values.numeric_values,
                        &arg_values.text_values,
                        metadata.and_then(|metadata| metadata.skill_type.as_deref()),
                        &legacy_pool_entries,
                    )
                })
            {
                rules.push(rule);
            }
            (!rules.is_empty()).then_some(rules)
        })
        .flatten()
        .collect::<Vec<_>>();
    let display_name =
        if character.name.trim().is_empty() { target_id } else { character.name.trim() };
    let mut base_character = character.clone();
    base_character.hp = stats.hp;
    base_character.max_hp = stats.max_hp;
    base_character.hp_regen = stats.hp_regen;
    base_character.mp = stats.mp;
    base_character.max_mp = stats.max_mp;
    base_character.mp_regen = stats.mp_regen;
    base_character.speed = stats.speed;
    base_character.damage_dealt_modifier = stats.damage_dealt_modifier;
    base_character.damage_taken_modifier = stats.damage_taken_modifier;
    base_character.healing_dealt_modifier = stats.healing_dealt_modifier;
    base_character.healing_taken_modifier = stats.healing_taken_modifier;
    base_character.extra_status = stats.extra_status.clone();
    rule_engine_state.sync_character(
        target_id,
        display_name,
        stats.hp,
        stats.max_hp,
        stats.mp,
        stats.max_mp,
        stats.hp_regen,
        stats.mp_regen,
        stats.speed,
        character_status_block(&character.status.combined(&stats.extra_status)),
        stats.damage_dealt_modifier,
        character_damage_attribute_multiplier(
            &base_character,
            stat_config,
            TrpgDamageBonusKind::Physical,
        ),
        character_damage_attribute_multiplier(
            &base_character,
            stat_config,
            TrpgDamageBonusKind::Magical,
        ),
        character_damage_attribute_multiplier(
            &base_character,
            stat_config,
            TrpgDamageBonusKind::Range,
        ),
        character_physical_damage_lifesteal(&base_character),
        character_physical_damage_followup_rate(&base_character),
        character_minimum_damage_floor(&base_character),
        character_chaos_output_variance(&base_character),
        character_fatigue_walker_available(&base_character),
        stats.damage_taken_modifier,
        character_large_hit_damage_taken_modifier(&base_character),
        character_damage_taken_attribute_multiplier(
            &base_character,
            TrpgDamageTakenKind::Magical,
        ),
        character_damage_taken_attribute_multiplier(
            &base_character,
            TrpgDamageTakenKind::Diseased,
        ),
        character_damage_taken_attribute_multiplier(
            &base_character,
            TrpgDamageTakenKind::Poisoning,
        ),
        stats.healing_dealt_modifier
            * character_healing_attribute_multiplier(&base_character, stat_config),
        character_wounded_healing_dealt_modifier(&base_character),
        character_mutual_aid_healing_rate(&base_character),
        stats.healing_taken_modifier,
        character_dying_target_healing_modifier(&base_character),
        character_damage_dealt_talent_buffs(&base_character, target_id),
        rules,
    );
}

fn legacy_moonberry_pool_entries(skill_pool: &[SkillPoolEntry]) -> Vec<LegacyMoonberryPoolEntry> {
    skill_pool
        .iter()
        .filter_map(|entry| {
            let legacy_json = entry.legacy_raw_payload()?;
            Some(LegacyMoonberryPoolEntry {
                id: entry.legacy_pool_id.clone(),
                name: skill_pool_entry_name(entry),
                legacy_json,
                args: entry
                    .args
                    .iter()
                    .map(|arg| LegacyMoonberryPoolArg {
                        name: arg.name.clone(),
                        kind: arg.kind.clone(),
                        value: arg.value.clone(),
                    })
                    .collect(),
            })
        })
        .collect()
}

fn character_status_block(status: &CharacterStatus) -> StatusBlock {
    StatusBlock {
        str_: status.str_,
        agi: status.agi,
        dex: status.dex,
        vit: status.vit,
        int_: status.int_,
        wis: status.wis,
        k: status.k,
        cha: status.cha,
    }
}

fn character_creation_step_options() -> [(CharacterCreationStep, &'static str); 14] {
    [
        (CharacterCreationStep::Normal, "普通"),
        (CharacterCreationStep::Str, "STR"),
        (CharacterCreationStep::Agi, "AGI"),
        (CharacterCreationStep::Dex, "DEX"),
        (CharacterCreationStep::Vit, "VIT"),
        (CharacterCreationStep::Int, "INT"),
        (CharacterCreationStep::Wis, "WIS"),
        (CharacterCreationStep::K, "K"),
        (CharacterCreationStep::Cha, "CHA"),
        (
            CharacterCreationStep::ConfirmStatus,
            "确认属性",
        ),
        (CharacterCreationStep::Skill, "技能"),
        (
            CharacterCreationStep::ConfirmSkill,
            "确认技能",
        ),
        (CharacterCreationStep::Image, "图片"),
        (CharacterCreationStep::Nickname, "昵称"),
    ]
}

fn character_creation_step_label(step: CharacterCreationStep) -> &'static str {
    character_creation_step_options()
        .iter()
        .find_map(|(candidate, label)| (*candidate == step).then_some(*label))
        .unwrap_or("未知")
}

fn character_status_source_ui(
    ui: &mut Ui,
    target_id: &str,
    character: &mut PlayerCharacter,
    edit_state: &mut CharacterEditState,
    unlocked: bool,
) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.label("属性来源");
        if unlocked {
            ui.small("GM修正草稿已解锁");
        } else {
            ui.small("已锁定");
        }
    });
    ui.small("创建值来自玩家建卡流程。GM修正值单独记录，并叠加到总值上。");

    if unlocked && !edit_state.gm_status_drafts.contains_key(target_id) {
        edit_state.gm_status_drafts.insert(
            target_id.to_owned(),
            character.extra_status.clone(),
        );
    }

    if unlocked {
        let draft_for_apply = {
            let draft = edit_state
                .gm_status_drafts
                .entry(target_id.to_owned())
                .or_insert_with(|| character.extra_status.clone());
            egui::Grid::new(ui.next_auto_id())
                .num_columns(5)
                .spacing([10.0, 4.0])
                .show(ui, |ui| {
                    ui.strong("属性");
                    ui.strong("创建");
                    ui.strong("当前GM");
                    ui.strong("草稿GM");
                    ui.strong("总值");
                    ui.end_row();
                    status_source_value_ui(
                        ui,
                        "STR",
                        character.status.str_,
                        character.extra_status.str_,
                        &mut draft.str_,
                    );
                    status_source_value_ui(
                        ui,
                        "AGI",
                        character.status.agi,
                        character.extra_status.agi,
                        &mut draft.agi,
                    );
                    status_source_value_ui(
                        ui,
                        "DEX",
                        character.status.dex,
                        character.extra_status.dex,
                        &mut draft.dex,
                    );
                    status_source_value_ui(
                        ui,
                        "VIT",
                        character.status.vit,
                        character.extra_status.vit,
                        &mut draft.vit,
                    );
                    status_source_value_ui(
                        ui,
                        "INT",
                        character.status.int_,
                        character.extra_status.int_,
                        &mut draft.int_,
                    );
                    status_source_value_ui(
                        ui,
                        "WIS",
                        character.status.wis,
                        character.extra_status.wis,
                        &mut draft.wis,
                    );
                    status_source_value_ui(
                        ui,
                        "K",
                        character.status.k,
                        character.extra_status.k,
                        &mut draft.k,
                    );
                    status_source_value_ui(
                        ui,
                        "CHA",
                        character.status.cha,
                        character.extra_status.cha,
                        &mut draft.cha,
                    );
                });
            draft.clone()
        };
        ui.horizontal(|ui| {
            if ui.button("应用GM修正").clicked() {
                character.extra_status = draft_for_apply.clone();
                edit_state.unlocked_status_targets.remove(target_id);
                edit_state.gm_status_drafts.remove(target_id);
                changed = true;
            }
            if ui.button("取消").clicked() {
                edit_state.unlocked_status_targets.remove(target_id);
                edit_state.gm_status_drafts.remove(target_id);
            }
        });
    } else {
        egui::Grid::new(ui.next_auto_id())
            .num_columns(4)
            .spacing([12.0, 4.0])
            .striped(true)
            .show(ui, |ui| {
                ui.strong("属性");
                ui.strong("创建");
                ui.strong("GM");
                ui.strong("总值");
                ui.end_row();
                readonly_status_source_row(
                    ui,
                    "STR",
                    character.status.str_,
                    character.extra_status.str_,
                );
                readonly_status_source_row(
                    ui,
                    "AGI",
                    character.status.agi,
                    character.extra_status.agi,
                );
                readonly_status_source_row(
                    ui,
                    "DEX",
                    character.status.dex,
                    character.extra_status.dex,
                );
                readonly_status_source_row(
                    ui,
                    "VIT",
                    character.status.vit,
                    character.extra_status.vit,
                );
                readonly_status_source_row(
                    ui,
                    "INT",
                    character.status.int_,
                    character.extra_status.int_,
                );
                readonly_status_source_row(
                    ui,
                    "WIS",
                    character.status.wis,
                    character.extra_status.wis,
                );
                readonly_status_source_row(
                    ui,
                    "K",
                    character.status.k,
                    character.extra_status.k,
                );
                readonly_status_source_row(
                    ui,
                    "CHA",
                    character.status.cha,
                    character.extra_status.cha,
                );
            });
    }

    changed
}

fn readonly_status_source_row(ui: &mut Ui, label: &str, creation: i32, gm: i32) {
    ui.label(label);
    ui.label(creation.to_string());
    ui.label(format_signed_status(gm));
    ui.label((creation + gm).to_string());
    ui.end_row();
}

fn status_source_value_ui(
    ui: &mut Ui,
    label: &str,
    creation: i32,
    current_gm: i32,
    draft_gm: &mut i32,
) -> bool {
    ui.label(label);
    ui.label(creation.to_string());
    ui.label(format_signed_status(current_gm));
    let response = ui.add(
        egui::DragValue::new(draft_gm)
            .range(-999..=999)
            .speed(1)
            .prefix("GM "),
    );
    ui.label((creation + *draft_gm).to_string());
    ui.end_row();
    response.changed()
}

fn format_signed_status(value: i32) -> String {
    if value > 0 {
        format!("+{value}")
    } else {
        value.to_string()
    }
}

fn character_hp_status(hp: f32, max_hp: f32) -> &'static str {
    if max_hp <= 0.0 {
        return "濒死";
    }
    if hp > max_hp * 0.8 {
        "无伤"
    } else if hp > max_hp * 0.6 {
        "轻伤"
    } else if hp > max_hp * 0.4 {
        "中伤"
    } else if hp > max_hp * 0.05 {
        "重伤"
    } else {
        "濒死"
    }
}

fn character_buff_editor_ui(
    ui: &mut Ui,
    target_id: &str,
    character: &mut PlayerCharacter,
    edit_state: &mut CharacterEditState,
    stat_config: &TrpgBasicConfig,
    rule_engine_state: &mut RuleEngineState,
    skill_pool: &[SkillPoolEntry],
) -> bool {
    let mut changed = false;
    let mut remove_index = None;

    sync_character_buffs(
        target_id,
        character,
        stat_config,
        rule_engine_state,
        skill_pool,
    );
    ui.horizontal_wrapped(|ui| {
        ui.label(format!(
            "生效buff：{}",
            character.active_buffs.len()
        ));
        let active_names = rule_engine_state.active_buff_names(target_id);
        if !active_names.is_empty() {
            ui.small(active_names.join(", "));
        }
    });

    for (index, buff) in character.active_buffs.iter_mut().enumerate() {
        ui.group(|ui| {
            ui.set_width(ui.available_width());
            ui.horizontal_wrapped(|ui| {
                changed |= ui.text_edit_singleline(&mut buff.name).changed();
                changed |= buff_kind_combo(ui, &mut buff.kind);
                let turns_response = ui.add(
                    egui::DragValue::new(&mut buff.turns_remaining)
                        .range(0..=999)
                        .prefix("轮数 "),
                );
                changed |= turns_response.on_hover_text("输入0为永久buff").changed();
                if buff.turns_remaining == 0 {
                    ui.small("永久");
                }
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut buff.priority)
                            .range(-999..=999)
                            .prefix("优先级 "),
                    )
                    .changed();
                changed |= ui.checkbox(&mut buff.beneficial, "增益").changed();
                if ui.button("移除").clicked() {
                    remove_index = Some(index);
                }
            });
            for effect in &buff.effects {
                ui.small(format_buff_effect(effect));
            }
        });
    }

    if let Some(index) = remove_index {
        character.active_buffs.remove(index);
        changed = true;
    }

    let draft = edit_state
        .buff_drafts
        .entry(target_id.to_owned())
        .or_default();
    ui.collapsing("给予buff", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("名称");
            ui.text_edit_singleline(&mut draft.name);
            buff_kind_combo(ui, &mut draft.kind);
            ui.add(
                egui::DragValue::new(&mut draft.turns_remaining)
                    .range(0..=999)
                    .prefix("轮数 "),
            )
            .on_hover_text("输入0为永久buff");
            ui.small("输入0为永久buff");
            ui.add(
                egui::DragValue::new(&mut draft.priority)
                    .range(-999..=999)
                    .prefix("优先级 "),
            );
            ui.checkbox(&mut draft.beneficial, "增益");
        });
        ui.horizontal_wrapped(|ui| {
            buff_field_combo(ui, &mut draft.field);
            buff_value_ui(ui, &mut draft.value);
        });
        if ui.button("应用buff").clicked() {
            let name = draft.name.trim();
            character.active_buffs.push(BuffSpec {
                name: if name.is_empty() { "未命名buff".to_owned() } else { name.to_owned() },
                kind: draft.kind,
                priority: draft.priority,
                turns_remaining: draft.turns_remaining.max(0),
                source_id: "gm".to_owned(),
                beneficial: draft.beneficial,
                effects: vec![BuffEffect {
                    field: draft.field,
                    value: draft.value,
                }],
                tick_actions: Vec::new(),
            });
            changed = true;
        }
    });

    if changed {
        sync_character_buffs(
            target_id,
            character,
            stat_config,
            rule_engine_state,
            skill_pool,
        );
    }
    changed
}

fn character_inventory_editor_ui(
    ui: &mut Ui,
    target_id: &str,
    character: &mut PlayerCharacter,
    edit_state: &mut CharacterEditState,
    skill_pool: &[SkillPoolEntry],
    item_pool: &[InventoryItem],
    default_open: bool,
) -> (bool, bool) {
    let mut changed = false;
    let mut equipment_changed = false;
    changed |= normalize_inventory(&mut character.inventory);
    changed |= normalize_character_hotbar(character);

    egui::CollapsingHeader::new("背包 / 装备")
        .default_open(default_open)
        .show(ui, |ui| {
            if !item_pool.is_empty() {
                let selected = edit_state
                    .item_pool_selected_index
                    .entry(target_id.to_owned())
                    .or_insert(0);
                if *selected >= item_pool.len() {
                    *selected = 0;
                }
                ui.horizontal_wrapped(|ui| {
                    egui::ComboBox::from_id_salt(("character_item_pool", target_id))
                        .selected_text(item_display_name(&item_pool[*selected]))
                        .show_ui(ui, |ui| {
                            for (index, item) in item_pool.iter().enumerate() {
                                ui.selectable_value(selected, index, item_display_name(item));
                            }
                        });
                    if ui.button("从物品池加入背包").clicked() {
                        add_item_to_inventory(
                            &mut character.inventory,
                            item_pool[*selected].clone(),
                        );
                        changed = true;
                    }
                });
            }
            ui.horizontal_wrapped(|ui| {
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut character.inventory.gold)
                            .range(0..=9_999_999)
                            .prefix("金币 "),
                    )
                    .changed();
                changed |= ui
                    .add(
                        egui::DragValue::new(&mut character.inventory.bag_slots)
                            .range(1..=160)
                            .prefix("格子 "),
                    )
                    .changed();
                ui.small(format!(
                    "已用 {}/{}",
                    character.inventory.items.len(),
                    character.inventory.bag_slots
                ));
            });

            ui.collapsing("已装备", |ui| {
                for slot in equipment_slot_options() {
                    if slot == EquipmentSlot::None {
                        continue;
                    }
                    ui.horizontal_wrapped(|ui| {
                        ui.label(equipment_slot_label(slot));
                        if let Some(item) = character.inventory.equipment.get(&slot) {
                            ui.colored_label(
                                item_quality_color(item.quality),
                                item_display_name(item),
                            );
                            ui.small(format!("物品等级 {}", item.item_level));
                            if !item.stat_effects.is_empty() {
                                ui.small(
                                    item.stat_effects
                                        .iter()
                                        .map(format_buff_effect)
                                        .collect::<Vec<_>>()
                                        .join("，"),
                                );
                            }
                            if ui.button("卸下").clicked() {
                                if let Some(item) = character.inventory.equipment.remove(&slot) {
                                    add_item_to_inventory(&mut character.inventory, item);
                                    changed = true;
                                    equipment_changed = true;
                                }
                            }
                        } else {
                            ui.small("空");
                        }
                    });
                }
            });

            ui.collapsing("生存快捷栏", |ui| {
                ui.small("GM可把背包物品或已批准的主动技能放进玩家的1-9快捷栏。");
                let active_skills = character_active_hotbar_skills(character, skill_pool);
                let active_skill_indexes = active_skills
                    .iter()
                    .map(|(index, _)| *index)
                    .collect::<HashSet<_>>();
                for slot in &mut character.inventory.hotbar {
                    if matches!(slot, CharacterHotbarSlot::Skill(index) if !active_skill_indexes.contains(index))
                    {
                        *slot = CharacterHotbarSlot::Empty;
                        changed = true;
                    }
                }
                for slot_index in 0..character.inventory.hotbar.len() {
                    let selected_text = character_hotbar_slot_label(
                        character.inventory.hotbar[slot_index],
                        character,
                    );
                    ui.horizontal(|ui| {
                        ui.label(format!("{}", slot_index + 1));
                        egui::ComboBox::from_id_salt(("character_hotbar", slot_index))
                            .selected_text(selected_text)
                            .width(180.0)
                            .show_ui(ui, |ui| {
                                changed |= ui
                                    .selectable_value(
                                        &mut character.inventory.hotbar[slot_index],
                                        CharacterHotbarSlot::Empty,
                                        "空",
                                    )
                                    .changed();
                                for item_index in 0..character.inventory.items.len() {
                                    let label =
                                        item_display_name(&character.inventory.items[item_index]);
                                    changed |= ui
                                        .selectable_value(
                                            &mut character.inventory.hotbar[slot_index],
                                            CharacterHotbarSlot::Item(item_index),
                                            format!("物品 · {label}"),
                                        )
                                        .changed();
                                }
                                for (skill_index, label) in &active_skills {
                                    changed |= ui
                                        .selectable_value(
                                            &mut character.inventory.hotbar[slot_index],
                                            CharacterHotbarSlot::Skill(*skill_index),
                                            format!("技能 · {label}"),
                                        )
                                        .changed();
                                }
                            });
                    });
                }
            });

            let mut remove_index = None;
            let mut equip_index = None;
            ui.horizontal(|ui| {
                ui.label("背包");
                if ui.button("+").on_hover_text("添加空物品").clicked() {
                    add_item_to_inventory(
                        &mut character.inventory,
                        InventoryItem::default(),
                    );
                    changed = true;
                }
            });
            egui::Grid::new(ui.next_auto_id())
                .num_columns(7)
                .spacing([8.0, 4.0])
                .striped(true)
                .show(ui, |ui| {
                    ui.strong("物品");
                    ui.strong("品质");
                    ui.strong("数量");
                    ui.strong("装备位");
                    ui.strong("等级");
                    ui.strong("绑定");
                    ui.strong("操作");
                    ui.end_row();

                    for (index, item) in character.inventory.items.iter_mut().enumerate() {
                        changed |= ui
                            .add(egui::TextEdit::singleline(&mut item.name).desired_width(120.0))
                            .changed();
                        changed |= item_quality_combo(ui, &mut item.quality);
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut item.stack)
                                    .range(1..=9999)
                                    .speed(1),
                            )
                            .changed();
                        changed |= equipment_slot_combo(ui, &mut item.equipment_slot);
                        changed |= ui
                            .add(
                                egui::DragValue::new(&mut item.item_level)
                                    .range(0..=9999)
                                    .speed(1),
                            )
                            .changed();
                        changed |= ui.checkbox(&mut item.soulbound, "").changed();
                        ui.horizontal(|ui| {
                            if ui
                                .add_enabled(
                                    item.equipment_slot != EquipmentSlot::None,
                                    egui::Button::new("装备"),
                                )
                                .clicked()
                            {
                                equip_index = Some(index);
                            }
                            if ui.button("-").on_hover_text("移除物品").clicked() {
                                remove_index = Some(index);
                            }
                        });
                        ui.end_row();
                    }
                });

            if !character.inventory.items.is_empty() {
                ui.collapsing("背包物品属性加成", |ui| {
                    for (index, item) in character.inventory.items.iter_mut().enumerate() {
                        ui.push_id(("inventory_item_stat_effects", index), |ui| {
                            ui.collapsing(
                                format!(
                                    "{} · {}项加成",
                                    item_display_name(item),
                                    item.stat_effects.len()
                                ),
                                |ui| {
                                    changed |= item_stat_effects_editor_ui(
                                        ui,
                                        &mut item.stat_effects,
                                    );
                                },
                            );
                        });
                    }
                });
            }

            if let Some(index) = equip_index {
                remove_character_inventory_item(character, index, true);
                changed = true;
                equipment_changed = true;
            }
            if let Some(index) = remove_index {
                remove_character_inventory_item(character, index, false);
                changed = true;
            }
        });

    changed |= normalize_inventory(&mut character.inventory);
    changed |= normalize_character_hotbar(character);
    (changed, equipment_changed)
}

fn buff_kind_combo(ui: &mut Ui, kind: &mut BuffKind) -> bool {
    let mut changed = false;
    egui::ComboBox::from_label("类型")
        .selected_text(buff_kind_label(*kind))
        .show_ui(ui, |ui| {
            for candidate in buff_kind_options() {
                changed |= ui
                    .selectable_value(
                        kind,
                        candidate,
                        buff_kind_label(candidate),
                    )
                    .changed();
            }
        });
    changed
}

fn normalize_inventory(inventory: &mut CharacterInventory) -> bool {
    let mut changed = false;
    if inventory.bag_slots == 0 {
        inventory.bag_slots = 1;
        changed = true;
    }
    if inventory.hotbar.len() != 9 {
        inventory.hotbar.resize(9, CharacterHotbarSlot::Empty);
        changed = true;
    }
    for item in &mut inventory.items {
        changed |= normalize_item(item);
    }
    for item in inventory.equipment.values_mut() {
        changed |= normalize_item(item);
    }
    let before_equipment = inventory.equipment.len();
    inventory
        .equipment
        .retain(|slot, item| *slot != EquipmentSlot::None && item.equipment_slot == *slot);
    changed |= inventory.equipment.len() != before_equipment;
    changed
}

fn normalize_character_hotbar(character: &mut PlayerCharacter) -> bool {
    let item_count = character.inventory.items.len();
    let skill_count = character.skill_names.len();
    let mut changed = false;
    for slot in &mut character.inventory.hotbar {
        let valid = match *slot {
            CharacterHotbarSlot::Empty => true,
            CharacterHotbarSlot::Item(index) => index < item_count,
            CharacterHotbarSlot::Skill(index) => index < skill_count,
        };
        if !valid {
            *slot = CharacterHotbarSlot::Empty;
            changed = true;
        }
    }
    changed
}

fn character_active_hotbar_skills(
    character: &mut PlayerCharacter,
    skill_pool: &[SkillPoolEntry],
) -> Vec<(usize, String)> {
    quick_cast_skills(character)
        .into_iter()
        .filter(|skill| {
            quick_cast_effect(
                &skill.note,
                &skill.arg_values,
                skill.skill_type.as_deref(),
                skill.legacy_buff_machine_json.as_deref(),
                skill_pool,
            )
            .is_some()
        })
        .map(|skill| (skill.index, skill.name))
        .collect()
}

fn character_hotbar_slot_label(slot: CharacterHotbarSlot, character: &PlayerCharacter) -> String {
    match slot {
        CharacterHotbarSlot::Empty => "空".to_owned(),
        CharacterHotbarSlot::Item(index) => character
            .inventory
            .items
            .get(index)
            .map(|item| format!("物品 · {}", item_display_name(item)))
            .unwrap_or_else(|| "空".to_owned()),
        CharacterHotbarSlot::Skill(index) => character
            .skill_names
            .get(index)
            .map(|name| {
                let name = name.trim();
                format!(
                    "技能 · {}",
                    if name.is_empty() { "未命名技能" } else { name }
                )
            })
            .unwrap_or_else(|| "空".to_owned()),
    }
}

fn character_hotbar_slot_short_label(
    slot: CharacterHotbarSlot,
    character: &PlayerCharacter,
) -> String {
    let label = match slot {
        CharacterHotbarSlot::Empty => return "空".to_owned(),
        CharacterHotbarSlot::Item(index) => {
            character.inventory.items.get(index).map(item_display_name)
        },
        CharacterHotbarSlot::Skill(index) => character.skill_names.get(index).cloned(),
    }
    .unwrap_or_else(|| "空".to_owned());
    let label = label.trim();
    if label.chars().count() <= 6 {
        label.to_owned()
    } else {
        format!(
            "{}…",
            label.chars().take(5).collect::<String>()
        )
    }
}

fn shift_character_hotbar_after_remove(
    hotbar: &mut [CharacterHotbarSlot],
    removed: CharacterHotbarSlot,
) {
    for slot in hotbar {
        *slot = match (*slot, removed) {
            (CharacterHotbarSlot::Item(index), CharacterHotbarSlot::Item(removed_index))
                if index == removed_index =>
            {
                CharacterHotbarSlot::Empty
            },
            (CharacterHotbarSlot::Item(index), CharacterHotbarSlot::Item(removed_index))
                if index > removed_index =>
            {
                CharacterHotbarSlot::Item(index - 1)
            },
            (CharacterHotbarSlot::Skill(index), CharacterHotbarSlot::Skill(removed_index))
                if index == removed_index =>
            {
                CharacterHotbarSlot::Empty
            },
            (CharacterHotbarSlot::Skill(index), CharacterHotbarSlot::Skill(removed_index))
                if index > removed_index =>
            {
                CharacterHotbarSlot::Skill(index - 1)
            },
            (current, _) => current,
        };
    }
}

fn remove_character_inventory_item(character: &mut PlayerCharacter, index: usize, equip: bool) {
    if index >= character.inventory.items.len() {
        return;
    }
    shift_character_hotbar_after_remove(
        &mut character.inventory.hotbar,
        CharacterHotbarSlot::Item(index),
    );
    if equip {
        equip_inventory_item(&mut character.inventory, index);
    } else {
        character.inventory.items.remove(index);
    }
}

fn normalize_item(item: &mut InventoryItem) -> bool {
    let mut changed = false;
    if item.max_stack == 0 {
        item.max_stack = 1;
        changed = true;
    }
    if item.stack == 0 {
        item.stack = 1;
        changed = true;
    }
    if item.stack > item.max_stack {
        item.stack = item.max_stack;
        changed = true;
    }
    changed
}

fn add_item_to_inventory(inventory: &mut CharacterInventory, mut item: InventoryItem) {
    normalize_item(&mut item);
    if !item.name.trim().is_empty() && item.max_stack > 1 {
        let mut remaining = item.stack;
        for existing in &mut inventory.items {
            if same_stackable_item(existing, &item) && existing.stack < existing.max_stack {
                let free = existing.max_stack - existing.stack;
                let moved = free.min(remaining);
                existing.stack += moved;
                remaining -= moved;
                if remaining == 0 {
                    return;
                }
            }
        }
        item.stack = remaining;
    }
    inventory.items.push(item);
}

fn same_stackable_item(left: &InventoryItem, right: &InventoryItem) -> bool {
    left.name == right.name
        && left.description == right.description
        && left.icon == right.icon
        && left.quality == right.quality
        && left.equipment_slot == right.equipment_slot
        && left.max_stack == right.max_stack
        && left.item_level == right.item_level
        && left.soulbound == right.soulbound
        && left.stat_effects == right.stat_effects
        && left.max_stack > 1
}

fn equip_inventory_item(inventory: &mut CharacterInventory, index: usize) {
    if index >= inventory.items.len() {
        return;
    }
    let item = inventory.items.remove(index);
    let slot = item.equipment_slot;
    if slot == EquipmentSlot::None {
        inventory.items.insert(index, item);
        return;
    }
    if let Some(previous) = inventory.equipment.insert(slot, item) {
        add_item_to_inventory(inventory, previous);
    }
}

fn item_display_name(item: &InventoryItem) -> String {
    if item.name.trim().is_empty() {
        "未命名物品".to_owned()
    } else if item.stack > 1 {
        format!("{} x{}", item.name.trim(), item.stack)
    } else {
        item.name.trim().to_owned()
    }
}

fn item_quality_combo(ui: &mut Ui, quality: &mut InventoryQuality) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(ui.next_auto_id())
        .selected_text(inventory_quality_label(*quality))
        .show_ui(ui, |ui| {
            for candidate in inventory_quality_options() {
                changed |= ui
                    .selectable_value(
                        quality,
                        candidate,
                        inventory_quality_label(candidate),
                    )
                    .changed();
            }
        });
    changed
}

fn equipment_slot_combo(ui: &mut Ui, slot: &mut EquipmentSlot) -> bool {
    let mut changed = false;
    egui::ComboBox::from_id_salt(ui.next_auto_id())
        .selected_text(equipment_slot_label(*slot))
        .show_ui(ui, |ui| {
            for candidate in equipment_slot_options() {
                changed |= ui
                    .selectable_value(
                        slot,
                        candidate,
                        equipment_slot_label(candidate),
                    )
                    .changed();
            }
        });
    changed
}

fn inventory_quality_options() -> [InventoryQuality; 6] {
    [
        InventoryQuality::Poor,
        InventoryQuality::Common,
        InventoryQuality::Uncommon,
        InventoryQuality::Rare,
        InventoryQuality::Epic,
        InventoryQuality::Legendary,
    ]
}

fn inventory_quality_label(quality: InventoryQuality) -> &'static str {
    match quality {
        InventoryQuality::Poor => "粗糙",
        InventoryQuality::Common => "普通",
        InventoryQuality::Uncommon => "优秀",
        InventoryQuality::Rare => "精良",
        InventoryQuality::Epic => "史诗",
        InventoryQuality::Legendary => "传说",
    }
}

fn item_quality_color(quality: InventoryQuality) -> egui::Color32 {
    match quality {
        InventoryQuality::Poor => egui::Color32::from_gray(150),
        InventoryQuality::Common => egui::Color32::WHITE,
        InventoryQuality::Uncommon => egui::Color32::from_rgb(30, 255, 0),
        InventoryQuality::Rare => egui::Color32::from_rgb(0, 112, 221),
        InventoryQuality::Epic => egui::Color32::from_rgb(163, 53, 238),
        InventoryQuality::Legendary => egui::Color32::from_rgb(255, 128, 0),
    }
}

fn equipment_slot_options() -> [EquipmentSlot; 16] {
    [
        EquipmentSlot::None,
        EquipmentSlot::Head,
        EquipmentSlot::Neck,
        EquipmentSlot::Shoulder,
        EquipmentSlot::Back,
        EquipmentSlot::Chest,
        EquipmentSlot::Wrist,
        EquipmentSlot::Hands,
        EquipmentSlot::Waist,
        EquipmentSlot::Legs,
        EquipmentSlot::Feet,
        EquipmentSlot::Finger,
        EquipmentSlot::Trinket,
        EquipmentSlot::MainHand,
        EquipmentSlot::OffHand,
        EquipmentSlot::Ranged,
    ]
}

fn equipment_slot_label(slot: EquipmentSlot) -> &'static str {
    match slot {
        EquipmentSlot::Head => "头部",
        EquipmentSlot::Neck => "颈部",
        EquipmentSlot::Shoulder => "肩部",
        EquipmentSlot::Back => "背部",
        EquipmentSlot::Chest => "胸部",
        EquipmentSlot::Wrist => "手腕",
        EquipmentSlot::Hands => "手",
        EquipmentSlot::Waist => "腰部",
        EquipmentSlot::Legs => "腿部",
        EquipmentSlot::Feet => "脚",
        EquipmentSlot::Finger => "戒指",
        EquipmentSlot::Trinket => "饰品",
        EquipmentSlot::MainHand => "主手",
        EquipmentSlot::OffHand => "副手",
        EquipmentSlot::Ranged => "远程",
        EquipmentSlot::None => "非装备",
    }
}

pub(crate) fn sync_character_buffs(
    target_id: &str,
    character: &mut PlayerCharacter,
    stat_config: &TrpgBasicConfig,
    rule_engine_state: &mut RuleEngineState,
    skill_pool: &[SkillPoolEntry],
) {
    let effective_buffs = character_effective_buffs(target_id, character);
    if effective_buffs.is_empty() {
        if let Some(base_stats) = character.buff_base_stats.take() {
            restore_character_base_stats(character, base_stats);
        }
        sync_character_skill_rules(
            target_id,
            character,
            stat_config,
            rule_engine_state,
            skill_pool,
        );
        rule_engine_state.replace_character_buffs(target_id, Vec::new());
        return;
    }

    if character.buff_base_stats.is_none() {
        character.buff_base_stats = Some(CharacterBuffBaseStats::from_character(
            character,
        ));
    }
    let base_stats = character
        .buff_base_stats
        .clone()
        .expect("buff base stats are initialized for active buffs");
    sync_character_skill_rules_with_stats(
        target_id,
        character,
        &base_stats,
        stat_config,
        rule_engine_state,
        skill_pool,
    );
    rule_engine_state.replace_character_buffs(target_id, effective_buffs);
    if let Some(effective) = rule_engine_state.character(target_id).cloned() {
        apply_effective_character_stats(
            character,
            &effective,
            &base_stats,
            stat_config,
        );
    }
}

fn character_effective_buffs(target_id: &str, character: &PlayerCharacter) -> Vec<BuffSpec> {
    let mut buffs = character.active_buffs.clone();
    buffs.extend(character_equipment_buffs(
        target_id, character,
    ));
    buffs.extend(character_legacy_passive_buffs(
        target_id, character,
    ));
    buffs.extend(character_moonberry_talent_passive_buffs(target_id, character));
    buffs
}

fn character_effect_sync_needed(target_id: &str, character: &PlayerCharacter) -> bool {
    let has_effects = !character.active_buffs.is_empty()
        || !character_equipment_buffs(target_id, character).is_empty()
        || !character_legacy_passive_buffs(target_id, character).is_empty()
        || !character_moonberry_talent_passive_buffs(target_id, character).is_empty();
    has_effects != character.buff_base_stats.is_some()
}

fn character_equipment_buffs(target_id: &str, character: &PlayerCharacter) -> Vec<BuffSpec> {
    let mut equipment = character.inventory.equipment.iter().collect::<Vec<_>>();
    equipment.sort_by_key(|(slot, _)| equipment_slot_sort_key(**slot));
    equipment
        .into_iter()
        .filter(|(_, item)| !item.stat_effects.is_empty())
        .map(|(slot, item)| BuffSpec {
            name: format!("装备 · {}", item_display_name(item)),
            kind: BuffKind::Physical,
            priority: 0,
            turns_remaining: 0,
            source_id: format!(
                "{target_id}:equipment:{}",
                equipment_slot_sort_key(*slot)
            ),
            beneficial: true,
            effects: item.stat_effects.clone(),
            tick_actions: Vec::new(),
        })
        .collect()
}

fn equipment_slot_sort_key(slot: EquipmentSlot) -> u8 {
    equipment_slot_options()
        .iter()
        .position(|candidate| *candidate == slot)
        .unwrap_or(usize::MAX) as u8
}

fn character_legacy_passive_buffs(target_id: &str, character: &PlayerCharacter) -> Vec<BuffSpec> {
    character
        .skill_metadata
        .iter()
        .enumerate()
        .filter(|(_, metadata)| metadata.is_approved())
        .flat_map(|(index, metadata)| {
            let Some(legacy_json) = metadata.legacy_buff_machine_json.as_deref() else {
                return Vec::new();
            };
            let source_id = format!("{target_id}:legacy-passive:{index}");
            legacy_moonberry_buff_machine_passive_buffs(
                legacy_json,
                &skill_rule_args(&metadata.args).numeric_values,
                &source_id,
            )
        })
        .collect()
}

fn character_moonberry_talent_passive_buffs(
    target_id: &str,
    character: &PlayerCharacter,
) -> Vec<BuffSpec> {
    let total_status = character.status.combined(&character.extra_status);
    character
        .skill_metadata
        .iter()
        .enumerate()
        .filter(|(_, metadata)| {
            metadata.is_approved() && metadata.source == CharacterSkillSourceKind::Talent
        })
        .filter_map(|(index, _)| {
            let talent_name = character.skill_names.get(index)?.trim();
            let effects = moonberry_talent_passive_effects(talent_name, &total_status);
            (!effects.is_empty()).then(|| BuffSpec {
                name: talent_name.to_owned(),
                kind: BuffKind::Magic,
                priority: 0,
                turns_remaining: 0,
                source_id: format!("{target_id}:talent-passive:{index}"),
                beneficial: true,
                effects,
                tick_actions: Vec::new(),
            })
        })
        .collect()
}

fn moonberry_talent_passive_effects(
    talent_name: &str,
    total_status: &CharacterStatus,
) -> Vec<BuffEffect> {
    match talent_name {
        "人类基因工程" => vec![BuffEffect {
            field: BuffField::MaxHp,
            value: BuffValue::AddPercent(5.0),
        }],
        "大魔法师" => vec![BuffEffect {
            field: BuffField::MaxMp,
            value: BuffValue::Add(total_status.int_ as f32),
        }],
        "矢量压缩能量池" => vec![
            BuffEffect {
                field: BuffField::MaxMp,
                value: BuffValue::Add(total_status.k as f32 * 2.0),
            },
            BuffEffect {
                field: BuffField::HealingDealtModifier,
                value: BuffValue::AddPercent(total_status.k as f32),
            },
        ],
        "狡黠之思" => vec![
            BuffEffect {
                field: BuffField::MaxMp,
                value: BuffValue::Add(total_status.wis as f32 * 2.0),
            },
            BuffEffect {
                field: BuffField::MpRegen,
                value: BuffValue::Add(total_status.wis as f32),
            },
        ],
        "狂风恶浪" => vec![BuffEffect {
            field: BuffField::Speed,
            value: BuffValue::AddPercent(20.0),
        }],
        "忏悔" => vec![BuffEffect {
            field: BuffField::HealingDealtModifier,
            value: BuffValue::AddPercent(25.0),
        }],
        _ => Vec::new(),
    }
}

fn capture_missing_group_initial_player_states(
    manager: &mut NapcatMessageManager,
    group_name: &str,
) -> usize {
    let Some(group) = manager.trpg_groups.get(group_name) else {
        return 0;
    };
    let players = group.players.clone();
    let snapshots = players
        .iter()
        .filter_map(|target_id| {
            manager
                .player_characters
                .get(target_id)
                .cloned()
                .map(|character| (target_id.clone(), character))
        })
        .collect::<Vec<_>>();
    let Some(group) = manager.trpg_groups.get_mut(group_name) else {
        return 0;
    };
    let mut captured = 0;
    group
        .initial_player_states
        .retain(|target_id, _| players.contains(target_id));
    for (target_id, character) in snapshots {
        if let std::collections::hash_map::Entry::Vacant(entry) =
            group.initial_player_states.entry(target_id)
        {
            entry.insert(character);
            captured += 1;
        }
    }
    captured
}

fn restore_character_initial_turn_stats(
    character: &mut PlayerCharacter,
    initial: &PlayerCharacter,
) {
    character.hp = initial.hp;
    character.max_hp = initial.max_hp;
    character.hp_regen = initial.hp_regen;
    character.mp = initial.mp;
    character.max_mp = initial.max_mp;
    character.mp_regen = initial.mp_regen;
    character.level = initial.level;
    character.exp = initial.exp;
    character.speed = initial.speed;
    character.damage_dealt_modifier = initial.damage_dealt_modifier;
    character.healing_dealt_modifier = initial.healing_dealt_modifier;
    character.damage_taken_modifier = initial.damage_taken_modifier;
    character.healing_taken_modifier = initial.healing_taken_modifier;
    character.damage_taken_this_turn = initial.damage_taken_this_turn;
    character.healing_taken_this_turn = initial.healing_taken_this_turn;
    character.status = initial.status.clone();
    character.extra_status = initial.extra_status.clone();
    character.skill_last_cast_turns = initial.skill_last_cast_turns.clone();
    character.skill_cooldown_ready_turns = initial.skill_cooldown_ready_turns.clone();
    character.active_buffs = initial.active_buffs.clone();
    character.buff_base_stats = initial.buff_base_stats.clone();
}

fn restore_group_initial_player_stats(
    manager: &mut NapcatMessageManager,
    group_name: &str,
    rule_engine_state: &mut RuleEngineState,
) -> (usize, usize) {
    let Some(group) = manager.trpg_groups.get(group_name) else {
        return (0, 0);
    };
    let players = group.players.clone();
    let initial_states = group.initial_player_states.clone();
    let skill_pool_snapshot = manager.skill_pool.clone();
    let mut restored = 0;
    let mut missing = 0;
    for target_id in players {
        let Some(initial) = initial_states.get(&target_id) else {
            missing += 1;
            continue;
        };
        let stat_config = manager.character_stat_config_for_target(&target_id);
        let Some(character) = manager.player_characters.get_mut(&target_id) else {
            missing += 1;
            continue;
        };
        restore_character_initial_turn_stats(character, initial);
        sync_character_buffs(
            &target_id,
            character,
            &stat_config,
            rule_engine_state,
            &skill_pool_snapshot,
        );
        restored += 1;
    }
    (restored, missing)
}

fn advance_group_world_turn(
    manager: &mut NapcatMessageManager,
    group_name: &str,
    rule_engine_state: &mut RuleEngineState,
) -> bool {
    let should_capture = manager
        .trpg_groups
        .get(group_name)
        .is_some_and(|group| group.world_turn == 0);
    let captured = if should_capture {
        capture_missing_group_initial_player_states(manager, group_name)
    } else {
        0
    };
    let Some(group) = manager.trpg_groups.get_mut(group_name) else {
        return false;
    };
    let players = group.players.clone();
    let advanced = group.advance_world_turn();
    let mut changed = captured > 0 || advanced;
    if advanced {
        changed |= reset_turn_totals_for_players(manager, &players);
        changed |= advance_buffs_for_players(manager, &players, rule_engine_state);
    }
    changed
}

fn mark_group_player_turn(
    manager: &mut NapcatMessageManager,
    group_name: &str,
    target_id: &str,
    acted: bool,
    rule_engine_state: &mut RuleEngineState,
) -> bool {
    let should_capture = manager
        .trpg_groups
        .get(group_name)
        .is_some_and(|group| group.world_turn == 0);
    let captured = if should_capture {
        capture_missing_group_initial_player_states(manager, group_name)
    } else {
        0
    };
    let Some(group) = manager.trpg_groups.get_mut(group_name) else {
        return false;
    };
    let previous_world_turn = group.world_turn;
    let players = group.players.clone();
    let changed = if acted {
        group.mark_player_acted(target_id)
    } else {
        group.mark_player_skipped(target_id)
    };
    if changed && group.world_turn > previous_world_turn {
        let _ = reset_turn_totals_for_players(manager, &players);
        advance_buffs_for_players(manager, &players, rule_engine_state);
    }
    captured > 0 || changed
}

fn set_group_player_waiting(
    manager: &mut NapcatMessageManager,
    group_name: &str,
    target_id: &str,
) -> bool {
    let Some(group) = manager.trpg_groups.get_mut(group_name) else {
        return false;
    };
    if !group.players.iter().any(|player_id| player_id == target_id) {
        return false;
    }
    group.sync_turn_players();
    let Some(turn) = group.player_turns.get_mut(target_id) else {
        return false;
    };
    if !turn.acted && !turn.skipped {
        return false;
    }

    turn.acted = false;
    turn.skipped = false;
    true
}

pub(crate) fn advance_buffs_for_players(
    manager: &mut NapcatMessageManager,
    players: &[String],
    rule_engine_state: &mut RuleEngineState,
) -> bool {
    let mut changed = false;
    let skill_pool_snapshot = manager.skill_pool.clone();
    let mut tick_actions = Vec::new();
    for target_id in players {
        let stat_config = manager.character_stat_config_for_target(target_id);
        let Some(character) = manager.player_characters.get_mut(target_id) else {
            continue;
        };
        let (advanced, ticks) = advance_character_buffs_with_ticks(target_id, character);
        tick_actions.extend(ticks);
        if advanced {
            sync_character_buffs(
                target_id,
                character,
                &stat_config,
                rule_engine_state,
                &skill_pool_snapshot,
            );
            changed = true;
        }
    }
    changed |= apply_character_buff_ticks(manager, &tick_actions);
    changed
}

fn reset_turn_totals_for_players(manager: &mut NapcatMessageManager, players: &[String]) -> bool {
    let mut changed = false;
    for target_id in players {
        if let Some(character) = manager.player_characters.get_mut(target_id) {
            changed |= reset_character_turn_totals(character);
        }
    }
    changed
}

#[cfg(test)]
fn advance_character_buffs(character: &mut PlayerCharacter) -> bool {
    advance_character_buffs_with_ticks("", character).0
}

#[derive(Debug, Clone)]
struct CharacterBuffTick {
    source_id: String,
    target_id: String,
    action: BuffTickAction,
}

fn advance_character_buffs_with_ticks(
    target_id: &str,
    character: &mut PlayerCharacter,
) -> (bool, Vec<CharacterBuffTick>) {
    if character.active_buffs.is_empty() {
        return (false, Vec::new());
    }

    let mut changed = false;
    let mut ticks = Vec::new();
    character.active_buffs.retain_mut(|buff| {
        if buff.turns_remaining == 0 {
            return true;
        }
        if buff.turns_remaining < 0 {
            changed = true;
            return false;
        }

        buff.turns_remaining -= 1;
        changed = true;
        if buff.turns_remaining > 0 {
            for action in &buff.tick_actions {
                ticks.push(CharacterBuffTick {
                    source_id: buff.source_id.clone(),
                    target_id: target_id.to_owned(),
                    action: action.clone(),
                });
            }
            true
        } else {
            false
        }
    });
    (changed, ticks)
}

fn apply_character_buff_ticks(
    manager: &mut NapcatMessageManager,
    ticks: &[CharacterBuffTick],
) -> bool {
    let mut changed = false;
    for tick in ticks {
        match &tick.action {
            BuffTickAction::Damage {
                amount,
                damage_type,
            } => {
                let stat_config = manager.character_stat_config_for_target(&tick.source_id);
                let source_multiplier = manager
                    .player_characters
                    .get(&tick.source_id)
                    .map(|source| {
                        source.damage_dealt_modifier
                            * character_low_hp_damage_multiplier(source)
                            * character_damage_attribute_multiplier(
                                source,
                                &stat_config,
                                trpg_damage_bonus_kind(*damage_type),
                            )
                            * moonberry_chaos_output_multiplier(character_chaos_output_variance(
                                source,
                            ))
                    })
                    .unwrap_or(1.0);
                let Some(target) = manager.player_characters.get_mut(&tick.target_id) else {
                    continue;
                };
                let target_multiplier = target.damage_taken_modifier
                    * character_damage_taken_attribute_multiplier(
                        target,
                        trpg_damage_taken_kind(*damage_type),
                    );
                let incoming_amount = (*amount * source_multiplier * target_multiplier).max(0.0);
                let final_amount = (incoming_amount
                    * large_hit_damage_taken_multiplier(
                        target.max_hp,
                        incoming_amount,
                        character_large_hit_damage_taken_modifier(target),
                    ))
                .max(0.0);
                changed |= apply_effective_character_damage(target, final_amount).0;
            },
            BuffTickAction::FixedDamage { amount, .. } => {
                let Some(target) = manager.player_characters.get_mut(&tick.target_id) else {
                    continue;
                };
                let final_amount = amount.max(0.0);
                changed |= apply_effective_character_damage(target, final_amount).0;
            },
            BuffTickAction::Heal { amount } => {
                let stat_config = manager.character_stat_config_for_target(&tick.source_id);
                let (
                    source_multiplier,
                    source_mutual_aid_healing_rate,
                    source_dying_target_healing_modifier,
                ) = manager
                    .player_characters
                    .get(&tick.source_id)
                    .map(|source| {
                        (
                            source.healing_dealt_modifier
                                * character_healing_attribute_multiplier(source, &stat_config)
                                * wounded_healing_dealt_multiplier(
                                    source.hp,
                                    source.max_hp,
                                    character_wounded_healing_dealt_modifier(source),
                                )
                                * moonberry_chaos_output_multiplier(
                                    character_chaos_output_variance(source),
                                ),
                            character_mutual_aid_healing_rate(source),
                            character_dying_target_healing_modifier(source),
                        )
                    })
                    .unwrap_or((1.0, 0.0, 1.0));
                let mut mutual_aid_heal = 0.0;
                {
                    let Some(target) = manager.player_characters.get_mut(&tick.target_id) else {
                        continue;
                    };
                    let target_multiplier = target.healing_taken_modifier
                        * dying_target_healing_multiplier(
                            target.hp,
                            target.max_hp,
                            source_dying_target_healing_modifier,
                        );
                    let final_amount = (*amount * source_multiplier * target_multiplier).max(0.0);
                    let target_mutual_aid_healing_rate = character_mutual_aid_healing_rate(target);
                    let (healing_changed, effective_amount) =
                        apply_effective_character_healing(target, final_amount);
                    changed |= healing_changed;
                    if tick.source_id != tick.target_id && effective_amount > f32::EPSILON {
                        mutual_aid_heal = effective_amount
                            * (source_mutual_aid_healing_rate + target_mutual_aid_healing_rate);
                    }
                };
                if mutual_aid_heal > f32::EPSILON {
                    if let Some(source) = manager.player_characters.get_mut(&tick.source_id) {
                        changed |= apply_effective_character_healing(source, mutual_aid_heal).0;
                    }
                }
            },
        }
    }
    changed
}

fn restore_character_base_stats(character: &mut PlayerCharacter, stats: CharacterBuffBaseStats) {
    character.hp = stats.hp;
    character.max_hp = stats.max_hp;
    character.hp_regen = stats.hp_regen;
    character.mp = stats.mp;
    character.max_mp = stats.max_mp;
    character.mp_regen = stats.mp_regen;
    character.speed = stats.speed;
    character.damage_dealt_modifier = stats.damage_dealt_modifier;
    character.damage_taken_modifier = stats.damage_taken_modifier;
    character.healing_dealt_modifier = stats.healing_dealt_modifier;
    character.healing_taken_modifier = stats.healing_taken_modifier;
    character.extra_status = stats.extra_status;
}

fn apply_effective_character_stats(
    character: &mut PlayerCharacter,
    effective: &RuleCharacter,
    base_stats: &CharacterBuffBaseStats,
    stat_config: &TrpgBasicConfig,
) {
    let base_total = character.status.combined(&base_stats.extra_status);
    let effective_total = character_status_from_block(&effective.status);
    let base_derived = derived_stats_for_total_status(
        character.level,
        &base_total,
        stat_config,
    );
    let effective_derived = derived_stats_for_total_status(
        character.level,
        &effective_total,
        stat_config,
    );

    character.hp = effective.hp;
    character.max_hp = (effective.max_hp + effective_derived.max_hp - base_derived.max_hp).max(0.0);
    character.hp_regen = effective.hp_regen + effective_derived.hp_regen - base_derived.hp_regen;
    character.mp = effective.mp;
    character.max_mp = (effective.max_mp + effective_derived.max_mp - base_derived.max_mp).max(0.0);
    character.mp_regen = effective.mp_regen + effective_derived.mp_regen - base_derived.mp_regen;
    character.speed = effective.speed + effective_derived.speed - base_derived.speed;
    character.extra_status = effective_extra_status(character, &effective.status);
    character.damage_dealt_modifier = effective.damage_dealt_modifier;
    character.damage_taken_modifier = effective.damage_taken_modifier;
    character.healing_dealt_modifier = effective.healing_dealt_modifier;
    character.healing_taken_modifier = effective.healing_taken_modifier;
    character.hp = character.hp.clamp(0.0, character.max_hp);
    character.mp = character.mp.clamp(0.0, character.max_mp);
}

#[derive(Clone, Copy)]
struct DerivedCharacterStats {
    max_hp: f32,
    hp_regen: f32,
    max_mp: f32,
    mp_regen: f32,
    speed: f32,
}

fn derived_stats_for_total_status(
    level: i32,
    total: &CharacterStatus,
    config: &TrpgBasicConfig,
) -> DerivedCharacterStats {
    DerivedCharacterStats {
        max_hp: (config.base_max_hp
            + level as f32 * config.lv_max_hp
            + total.str_ as f32 * config.str_max_hp
            + total.vit as f32 * config.vit_max_hp)
            .max(1.0),
        hp_regen: total.vit.max(0) as f32 * config.vit_hp_reg,
        max_mp: total.int_ as f32 * config.int_max_mp + total.wis as f32 * config.wis_max_mp,
        mp_regen: total.wis.max(0) as f32
            * config.wis_mp_reg
            * (1.0 + config.weave_mp_regen_bonus.max(0.0)),
        speed: config.basic_speed
            + total.str_.max(0) as f32 * config.str_speed
            + total.agi.max(0) as f32 * config.agi_speed
            + total.dex.max(0) as f32 * config.dex_speed,
    }
}

fn character_status_from_block(status: &StatusBlock) -> CharacterStatus {
    CharacterStatus {
        str_: status.str_,
        agi: status.agi,
        dex: status.dex,
        vit: status.vit,
        int_: status.int_,
        wis: status.wis,
        k: status.k,
        cha: status.cha,
    }
}

fn effective_extra_status(
    character: &PlayerCharacter,
    effective_status: &StatusBlock,
) -> CharacterStatus {
    CharacterStatus {
        str_: effective_status.str_ - character.status.str_,
        agi: effective_status.agi - character.status.agi,
        dex: effective_status.dex - character.status.dex,
        vit: effective_status.vit - character.status.vit,
        int_: effective_status.int_ - character.status.int_,
        wis: effective_status.wis - character.status.wis,
        k: effective_status.k - character.status.k,
        cha: effective_status.cha - character.status.cha,
    }
}

fn buff_kind_options() -> [BuffKind; 6] {
    [
        BuffKind::Magic,
        BuffKind::Curse,
        BuffKind::Disease,
        BuffKind::Bleed,
        BuffKind::Poison,
        BuffKind::None,
    ]
}

fn buff_kind_label(kind: BuffKind) -> &'static str {
    match kind {
        BuffKind::None => "无",
        BuffKind::Magic => "魔法",
        BuffKind::Physical => "无",
        BuffKind::Curse => "诅咒",
        BuffKind::Disease => "疾病",
        BuffKind::Bleed => "流血",
        BuffKind::Range => "无",
        BuffKind::Poison => "中毒",
    }
}

fn buff_field_combo(ui: &mut Ui, field: &mut BuffField) {
    egui::ComboBox::from_label("字段")
        .selected_text(buff_field_label(*field))
        .show_ui(ui, |ui| {
            for candidate in buff_field_options() {
                ui.selectable_value(
                    field,
                    candidate,
                    buff_field_label(candidate),
                );
            }
        });
}

fn buff_value_ui(ui: &mut Ui, value: &mut BuffValue) {
    let mut mode = match value {
        BuffValue::Add(_) => 0,
        BuffValue::AddPercent(_) => 1,
        BuffValue::Set(_) => 2,
        BuffValue::SetPercentOfBase(_) => 3,
    };
    egui::ComboBox::from_label("数值")
        .selected_text(buff_value_mode_label(mode))
        .show_ui(ui, |ui| {
            ui.selectable_value(&mut mode, 0, "增加");
            ui.selectable_value(&mut mode, 1, "增加%");
            ui.selectable_value(&mut mode, 2, "设为");
            ui.selectable_value(&mut mode, 3, "设为基础%");
        });

    let mut amount = match *value {
        BuffValue::Add(amount)
        | BuffValue::AddPercent(amount)
        | BuffValue::Set(amount)
        | BuffValue::SetPercentOfBase(amount) => amount,
    };
    ui.add(
        egui::DragValue::new(&mut amount)
            .speed(0.1)
            .range(-9999.0..=9999.0),
    );
    *value = match mode {
        0 => BuffValue::Add(amount),
        1 => BuffValue::AddPercent(amount),
        2 => BuffValue::Set(amount),
        _ => BuffValue::SetPercentOfBase(amount),
    };
}

fn buff_field_options() -> [BuffField; 19] {
    [
        BuffField::Hp,
        BuffField::Mp,
        BuffField::MaxHp,
        BuffField::MaxMp,
        BuffField::HpRegen,
        BuffField::MpRegen,
        BuffField::Speed,
        BuffField::Status(StatusKey::Str),
        BuffField::Status(StatusKey::Agi),
        BuffField::Status(StatusKey::Dex),
        BuffField::Status(StatusKey::Vit),
        BuffField::Status(StatusKey::Int),
        BuffField::Status(StatusKey::Wis),
        BuffField::Status(StatusKey::K),
        BuffField::Status(StatusKey::Cha),
        BuffField::DamageDealtModifier,
        BuffField::DamageTakenModifier,
        BuffField::HealingDealtModifier,
        BuffField::HealingTakenModifier,
    ]
}

fn buff_field_label(field: BuffField) -> &'static str {
    match field {
        BuffField::Hp => "HP",
        BuffField::Mp => "MP",
        BuffField::MaxHp => "最大HP",
        BuffField::MaxMp => "最大MP",
        BuffField::HpRegen => "HP回复",
        BuffField::MpRegen => "MP回复",
        BuffField::Speed => "移动速度",
        BuffField::Status(StatusKey::Str) => "STR",
        BuffField::Status(StatusKey::Agi) => "AGI",
        BuffField::Status(StatusKey::Dex) => "DEX",
        BuffField::Status(StatusKey::Vit) => "VIT",
        BuffField::Status(StatusKey::Int) => "INT",
        BuffField::Status(StatusKey::Wis) => "WIS",
        BuffField::Status(StatusKey::K) => "K",
        BuffField::Status(StatusKey::Cha) => "CHA",
        BuffField::DamageDealtModifier => "造成伤害",
        BuffField::DamageTakenModifier => "受到伤害",
        BuffField::HealingDealtModifier => "造成治疗",
        BuffField::HealingTakenModifier => "受到治疗",
    }
}

fn buff_value_mode_label(mode: i32) -> &'static str {
    match mode {
        0 => "增加",
        1 => "增加%",
        2 => "设为",
        _ => "设为基础%",
    }
}

fn format_buff_effect(effect: &BuffEffect) -> String {
    let value = match effect.value {
        BuffValue::Add(amount) => format!("+{}", format_character_number(amount)),
        BuffValue::AddPercent(amount) => format!("+{}%", format_character_number(amount)),
        BuffValue::Set(amount) => format!("={}", format_character_number(amount)),
        BuffValue::SetPercentOfBase(amount) => {
            format!(
                "{}%基础",
                format_character_number(amount)
            )
        },
    };
    format!(
        "{} {}",
        buff_field_label(effect.field),
        value
    )
}

const RANDOM_POOL_FILTER_ALL: &str = "__all__";
const RANDOM_POOL_FILTER_UNGROUPED: &str = "__ungrouped__";
const RANDOM_POOL_FILTER_UNTAGGED: &str = "__untagged__";

fn random_pool_tag_tokens(tags: &str) -> Vec<String> {
    let mut tokens = tags
        .split_whitespace()
        .map(str::trim)
        .filter(|tag| !tag.is_empty())
        .map(str::to_owned)
        .collect::<Vec<_>>();
    tokens.sort();
    tokens.dedup();
    tokens
}

fn random_pool_group_filter_value(group: Option<i32>) -> String {
    group
        .map(|group| group.to_string())
        .unwrap_or_else(|| RANDOM_POOL_FILTER_UNGROUPED.to_owned())
}

fn random_pool_group_label(group: Option<i32>) -> String {
    match group {
        Some(group) if group > 10000 => format!("QQ {}", group),
        Some(group) => format!("旧团索引 {}", group),
        None => "未分组".to_owned(),
    }
}

fn random_pool_filter_options(manager: &NapcatMessageManager) -> (Vec<Option<i32>>, Vec<String>) {
    let mut groups = manager
        .random_pools
        .values()
        .map(|pool| pool.legacy_group)
        .collect::<Vec<_>>();
    groups.sort();
    groups.dedup();

    let mut tags = manager
        .random_pools
        .values()
        .flat_map(|pool| random_pool_tag_tokens(&pool.tags))
        .collect::<Vec<_>>();
    tags.sort();
    tags.dedup();

    (groups, tags)
}

fn random_pool_matches_filters(pool: &RandomPool, group_filter: &str, tag_filter: &str) -> bool {
    let group_matches = group_filter.is_empty()
        || group_filter == RANDOM_POOL_FILTER_ALL
        || group_filter == random_pool_group_filter_value(pool.legacy_group);
    let tag_matches = tag_filter.is_empty()
        || tag_filter == RANDOM_POOL_FILTER_ALL
        || (tag_filter == RANDOM_POOL_FILTER_UNTAGGED
            && random_pool_tag_tokens(&pool.tags).is_empty())
        || random_pool_tag_tokens(&pool.tags)
            .iter()
            .any(|tag| tag == tag_filter);

    group_matches && tag_matches
}

fn random_pool_metadata_summary(pool: &RandomPool) -> String {
    let mut parts = vec![random_pool_group_label(pool.legacy_group)];
    let tags = random_pool_tag_tokens(&pool.tags);
    if !tags.is_empty() {
        parts.push(format!("标签 {}", tags.join(" ")));
    }
    if !pool.created_at.trim().is_empty() {
        parts.push(format!(
            "创建 {}",
            pool.created_at.trim()
        ));
    }
    parts.join(" · ")
}

fn random_pool_metadata_editor_ui(ui: &mut Ui, pool: &mut RandomPool) -> bool {
    let mut changed = false;
    ui.collapsing("月莓旧随机池元数据", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label("旧ID");
            let clear_legacy_pool_id = {
                let legacy_pool_id = pool.legacy_pool_id.get_or_insert_with(String::new);
                changed |= ui
                    .add(egui::TextEdit::singleline(legacy_pool_id).desired_width(120.0))
                    .changed();
                legacy_pool_id.trim().is_empty()
            };
            if clear_legacy_pool_id && pool.legacy_pool_id.is_some() {
                pool.legacy_pool_id = None;
                changed = true;
            }

            let mut has_group = pool.legacy_group.is_some();
            if ui.checkbox(&mut has_group, "旧分组").changed() {
                pool.legacy_group = has_group.then_some(0);
                changed = true;
            }
            if let Some(group) = pool.legacy_group.as_mut() {
                changed |= ui
                    .add(
                        egui::DragValue::new(group)
                            .range(0..=999_999)
                            .speed(1)
                            .prefix("编号 "),
                    )
                    .changed();
            }

            ui.label("创建");
            changed |= ui
                .add(egui::TextEdit::singleline(&mut pool.created_at).desired_width(140.0))
                .changed();
        });
        ui.label("标签（空格分隔）");
        changed |= ui
            .add(
                egui::TextEdit::singleline(&mut pool.tags)
                    .desired_width(ui.available_width().min(CHARACTER_FIELD_MAX_WIDTH)),
            )
            .changed();
        ui.label("描述");
        changed |= ui
            .add(
                egui::TextEdit::multiline(&mut pool.description)
                    .desired_rows(2)
                    .desired_width(ui.available_width().min(CHARACTER_FIELD_MAX_WIDTH)),
            )
            .changed();
    });

    changed
}

fn random_pool_settings_ui(
    ui: &mut Ui,
    manager: &mut NapcatMessageManager,
    state: &mut TrpgGroupSettingsState,
    player_targets: &[String],
    napcat_sender: Option<&NapcatIOSender>,
    mut ime: Option<&mut ImeManager>,
) -> bool {
    let mut changed = false;

    ui.heading("随机池");
    ui.horizontal_wrapped(|ui| {
        ui.label("池名");
        ui.text_edit_singleline(&mut state.new_random_pool_name);
        if ui.button("创建随机池").clicked() {
            let name = state.new_random_pool_name.trim();
            if !name.is_empty() {
                manager
                    .random_pools
                    .entry(name.to_owned())
                    .or_insert_with(RandomPool::default);
                state.new_random_pool_name.clear();
                changed = true;
            }
        }

        if !player_targets.is_empty() {
            if state.random_pool_award_target.is_empty()
                || !player_targets
                    .iter()
                    .any(|target_id| target_id == &state.random_pool_award_target)
            {
                state.random_pool_award_target = player_targets[0].clone();
            }
            egui::ComboBox::from_label("发给角色")
                .selected_text(target_display_name(
                    manager,
                    &state.random_pool_award_target,
                ))
                .show_ui(ui, |ui| {
                    for target_id in player_targets {
                        ui.selectable_value(
                            &mut state.random_pool_award_target,
                            target_id.clone(),
                            target_display_name(manager, target_id),
                        );
                    }
                });
        }
    });

    let (group_filter_options, tag_filter_options) = random_pool_filter_options(manager);
    if state.random_pool_group_filter.is_empty() {
        state.random_pool_group_filter = RANDOM_POOL_FILTER_ALL.to_owned();
    }
    if state.random_pool_tag_filter.is_empty() {
        state.random_pool_tag_filter = RANDOM_POOL_FILTER_ALL.to_owned();
    }
    ui.horizontal_wrapped(|ui| {
        egui::ComboBox::from_label("旧分组筛选")
            .selected_text(
                if state.random_pool_group_filter == RANDOM_POOL_FILTER_ALL {
                    "全部".to_owned()
                } else if state.random_pool_group_filter == RANDOM_POOL_FILTER_UNGROUPED {
                    "未分组".to_owned()
                } else {
                    state.random_pool_group_filter.clone()
                },
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut state.random_pool_group_filter,
                    RANDOM_POOL_FILTER_ALL.to_owned(),
                    "全部",
                );
                for group in &group_filter_options {
                    ui.selectable_value(
                        &mut state.random_pool_group_filter,
                        random_pool_group_filter_value(*group),
                        random_pool_group_label(*group),
                    );
                }
            });
        egui::ComboBox::from_label("标签筛选")
            .selected_text(
                if state.random_pool_tag_filter == RANDOM_POOL_FILTER_ALL {
                    "全部".to_owned()
                } else if state.random_pool_tag_filter == RANDOM_POOL_FILTER_UNTAGGED {
                    "无标签".to_owned()
                } else {
                    state.random_pool_tag_filter.clone()
                },
            )
            .show_ui(ui, |ui| {
                ui.selectable_value(
                    &mut state.random_pool_tag_filter,
                    RANDOM_POOL_FILTER_ALL.to_owned(),
                    "全部",
                );
                ui.selectable_value(
                    &mut state.random_pool_tag_filter,
                    RANDOM_POOL_FILTER_UNTAGGED.to_owned(),
                    "无标签",
                );
                for tag in &tag_filter_options {
                    ui.selectable_value(
                        &mut state.random_pool_tag_filter,
                        tag.clone(),
                        tag,
                    );
                }
            });
    });

    let current_group_name = manager.current_trpg_group.clone();
    let current_group_snapshot = manager.current_group().cloned();
    let mut send_targets = Vec::new();
    let mut send_scope = String::new();
    let mut send_scope_label = String::new();
    let mut has_send_scope = false;
    if ime.is_some() {
        if state.random_pool_batch_count == 0 {
            state.random_pool_batch_count = 1;
        }
        ui.horizontal_wrapped(|ui| {
            ui.label("批量文本");
            changed |= ui
                .add(
                    egui::DragValue::new(&mut state.random_pool_batch_count)
                        .range(1..=RANDOM_POOL_BATCH_MAX)
                        .speed(1)
                        .prefix("抽取 "),
                )
                .changed();
            state.random_pool_batch_count = state
                .random_pool_batch_count
                .clamp(1, RANDOM_POOL_BATCH_MAX);
            if napcat_sender.is_none() {
                ui.small("NapCat websocket未连接");
            }
        });

        if let (Some(group_name), Some(current_group)) = (
            current_group_name.as_deref(),
            current_group_snapshot.as_ref(),
        ) {
            if state.random_pool_broadcast_scope.is_empty() {
                state.random_pool_broadcast_scope = BROADCAST_SCOPE_ALL.to_owned();
            }
            group_broadcast_scope_ui(
                ui,
                group_name,
                &current_group.players,
                Some(current_group),
                &mut state.random_pool_broadcast_scope,
            );
            send_scope = state.random_pool_broadcast_scope.clone();
            send_scope_label = broadcast_scope_label(Some(current_group), &send_scope);
            send_targets = group_broadcast_targets(
                Some(current_group),
                &current_group.players,
                manager,
                &send_scope,
            );
            has_send_scope = true;
        } else {
            ui.small("选择当前TRPG组后可批量发送文本结果。");
        }

        if !state.random_pool_send_status.is_empty() {
            ui.small(&state.random_pool_send_status);
        }
    }

    let mut pool_names = manager
        .random_pools
        .iter()
        .filter(|(_, pool)| {
            random_pool_matches_filters(
                pool,
                &state.random_pool_group_filter,
                &state.random_pool_tag_filter,
            )
        })
        .map(|(pool_name, _)| pool_name.clone())
        .collect::<Vec<_>>();
    pool_names.sort();
    if manager.random_pools.is_empty() {
        ui.label("还没有随机池。");
        return changed;
    } else if pool_names.is_empty() {
        ui.label("当前筛选下没有随机池。");
        return changed;
    }

    let mut pool_to_delete = None;
    for pool_name in pool_names {
        let checked_target_labels = manager
            .random_pools
            .get(&pool_name)
            .map(|pool| {
                pool.checked_results
                    .iter()
                    .map(|result| {
                        (
                            result.target_id.clone(),
                            target_display_name(manager, &result.target_id),
                        )
                    })
                    .collect::<HashMap<_, _>>()
            })
            .unwrap_or_default();
        let Some(pool) = manager.random_pools.get_mut(&pool_name) else {
            continue;
        };
        let total_weight = random_pool_total_weight(pool);
        ui.collapsing(
            format!("{pool_name} ({})", pool.entries.len()),
            |ui| {
                ui.small(random_pool_metadata_summary(pool));
                changed |= random_pool_metadata_editor_ui(ui, pool);

                ui.horizontal_wrapped(|ui| {
                    if ui.button("随机抽取").clicked() {
                        if let Some(entry) = pick_random_pool_entry(pool) {
                            let mut item = entry.item.clone();
                            normalize_item(&mut item);
                            pool.last_pick = Some(item.clone());
                            pool.last_text_result = random_pool_entry_text_result(&entry);
                            if let Some(character) = manager
                                .player_characters
                                .get_mut(&state.random_pool_award_target)
                            {
                                add_item_to_inventory(&mut character.inventory, item);
                            }
                            changed = true;
                        }
                    }
                    if ime.is_some() {
                        let send_enabled = napcat_sender.is_some()
                            && has_send_scope
                            && !send_targets.is_empty()
                            && total_weight > 0.0;
                        let response = ui.add_enabled(
                            send_enabled,
                            egui::Button::new("批量抽取并发送"),
                        );
                        let clicked = response.clicked();
                        let hover_text = if !has_send_scope {
                            "先选择当前TRPG组"
                        } else if send_targets.is_empty() {
                            "当前范围没有可发送的玩家私聊"
                        } else if total_weight <= 0.0 {
                            "随机池没有可抽取的启用项目"
                        } else if napcat_sender.is_none() {
                            "NapCat websocket未连接"
                        } else {
                            "抽取文本结果并发送到当前范围"
                        };
                        response.on_hover_text(hover_text);
                        if clicked {
                            let results =
                                random_pool_batch_text_results(pool, state.random_pool_batch_count);
                            if let Some(message) =
                                random_pool_text_results_message(&pool_name, &results)
                            {
                                if let (Some(sender), Some(ime)) =
                                    (napcat_sender, ime.as_deref_mut())
                                {
                                    let input_id =
                                        random_pool_send_input_id(&pool_name, &send_scope);
                                    match ime.queue_text_send(
                                        &input_id,
                                        &message,
                                        sender,
                                        send_targets.clone(),
                                    ) {
                                        Ok(()) => {
                                            pool.last_text_result = results.last().cloned();
                                            state.random_pool_send_status = format!(
                                                "已入队：{}，{}个目标，{}条结果",
                                                send_scope_label,
                                                send_targets.len(),
                                                results.len()
                                            );
                                            changed = true;
                                        },
                                        Err(err) => {
                                            state.random_pool_send_status =
                                                format!("发送失败：{err}");
                                        },
                                    }
                                }
                            } else {
                                state.random_pool_send_status =
                                    "随机池没有可发送的文本结果。".to_owned();
                            }
                        }

                        let stage_enabled =
                            has_send_scope && !send_targets.is_empty() && total_weight > 0.0;
                        let response = ui.add_enabled(
                            stage_enabled,
                            egui::Button::new("生成待发送结果"),
                        );
                        let clicked = response.clicked();
                        response.on_hover_text(if !has_send_scope {
                            "先选择当前TRPG组"
                        } else if send_targets.is_empty() {
                            "当前范围没有可发送的玩家私聊"
                        } else if total_weight <= 0.0 {
                            "随机池没有可抽取的启用项目"
                        } else {
                            "生成可逐条确认的玩家私聊随机结果"
                        });
                        if clicked {
                            pool.checked_results = random_pool_checked_results(pool, &send_targets);
                            if pool.checked_results.is_empty() {
                                state.random_pool_send_status =
                                    "没有生成可发送的随机结果。".to_owned();
                            } else {
                                state.random_pool_send_status = format!(
                                    "已生成：{}，{}条待确认结果",
                                    send_scope_label,
                                    pool.checked_results.len()
                                );
                            }
                            changed = true;
                        }

                        let (enabled_checked, total_checked) =
                            random_pool_checked_result_summary(pool);
                        let response = ui.add_enabled(
                            napcat_sender.is_some() && enabled_checked > 0,
                            egui::Button::new("发送勾选结果"),
                        );
                        let clicked = response.clicked();
                        response.on_hover_text(if total_checked == 0 {
                            "先生成待发送结果"
                        } else if enabled_checked == 0 {
                            "没有已勾选且有内容的结果"
                        } else if napcat_sender.is_none() {
                            "NapCat websocket未连接"
                        } else {
                            "逐条发送勾选的玩家私聊结果"
                        });
                        if clicked {
                            if let (Some(sender), Some(ime)) = (napcat_sender, ime.as_deref_mut()) {
                                match queue_random_pool_checked_results(
                                    &pool_name, pool, sender, ime,
                                ) {
                                    Ok(sent_count) => {
                                        for result in pool.checked_results.iter_mut() {
                                            if result.enabled && !result.text.trim().is_empty() {
                                                result.enabled = false;
                                            }
                                        }
                                        state.random_pool_send_status =
                                            format!("已入队：{}条勾选结果", sent_count);
                                        changed = true;
                                    },
                                    Err(err) => {
                                        state.random_pool_send_status = format!("发送失败：{err}");
                                    },
                                }
                            }
                        }

                        if ui.button("清空待发送").clicked() {
                            pool.checked_results.clear();
                            changed = true;
                        }
                    }
                    if let Some(item) = pool.last_pick.as_ref() {
                        ui.label("上次抽取");
                        ui.colored_label(
                            item_quality_color(item.quality),
                            item_display_name(item),
                        );
                    }
                    if let Some(result) = pool.last_text_result.as_ref() {
                        ui.label("上次文本");
                        ui.label(random_pool_text_result_label(result));
                    }
                    if ui.button("删除池").clicked() {
                        pool_to_delete = Some(pool_name.clone());
                    }
                });

                if !pool.checked_results.is_empty() {
                    ui.separator();
                    let (enabled_checked, total_checked) = random_pool_checked_result_summary(pool);
                    ui.label(format!(
                        "待发送结果：{} / {} 条已勾选",
                        enabled_checked, total_checked
                    ));
                    let mut remove_checked_index = None;
                    egui::Grid::new((
                        ui.next_auto_id(),
                        "random_pool_checked_results",
                    ))
                    .num_columns(4)
                    .spacing([8.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("发送");
                        ui.strong("目标");
                        ui.strong("内容");
                        ui.strong("操作");
                        ui.end_row();

                        for (index, result) in pool.checked_results.iter_mut().enumerate() {
                            changed |= ui.checkbox(&mut result.enabled, "").changed();
                            ui.label(
                                checked_target_labels
                                    .get(&result.target_id)
                                    .cloned()
                                    .unwrap_or_else(|| result.target_id.clone()),
                            )
                            .on_hover_text(&result.target_id);
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut result.text)
                                        .desired_width(240.0),
                                )
                                .changed();
                            if ui.button("-").on_hover_text("移除待发送结果").clicked() {
                                remove_checked_index = Some(index);
                            }
                            ui.end_row();
                        }
                    });
                    if let Some(index) = remove_checked_index {
                        pool.checked_results.remove(index);
                        changed = true;
                    }
                }

                let mut remove_index = None;
                egui::Grid::new(ui.next_auto_id())
                    .num_columns(11)
                    .spacing([8.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("启用");
                        ui.strong("物品");
                        ui.strong("文本结果");
                        ui.strong("最小");
                        ui.strong("最大");
                        ui.strong("品质");
                        ui.strong("权重");
                        ui.strong("概率");
                        ui.strong("数量");
                        ui.strong("装备位");
                        ui.strong("操作");
                        ui.end_row();

                        for (index, entry) in pool.entries.iter_mut().enumerate() {
                            changed |= ui.checkbox(&mut entry.enabled, "").changed();
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut entry.item.name)
                                        .desired_width(120.0),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::TextEdit::singleline(&mut entry.result_text)
                                        .desired_width(140.0),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut entry.min_count)
                                        .range(0..=9999)
                                        .speed(1),
                                )
                                .changed();
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut entry.max_count)
                                        .range(0..=9999)
                                        .speed(1),
                                )
                                .changed();
                            changed |= normalize_random_pool_entry(entry);
                            changed |= item_quality_combo(ui, &mut entry.item.quality);
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut entry.weight)
                                        .range(0.0..=999_999.0)
                                        .speed(0.1),
                                )
                                .changed();
                            let probability = if entry.enabled && total_weight > 0.0 {
                                entry.weight.max(0.0) / total_weight * 100.0
                            } else {
                                0.0
                            };
                            ui.label(format!("{probability:.1}%"));
                            changed |= ui
                                .add(
                                    egui::DragValue::new(&mut entry.item.stack)
                                        .range(1..=9999)
                                        .speed(1),
                                )
                                .changed();
                            changed |= equipment_slot_combo(ui, &mut entry.item.equipment_slot);
                            if ui.button("-").on_hover_text("移除池项目").clicked() {
                                remove_index = Some(index);
                            }
                            ui.end_row();
                        }
                    });

                if let Some(index) = remove_index {
                    pool.entries.remove(index);
                    changed = true;
                }

                let draft = state
                    .random_pool_entry_drafts
                    .entry(pool_name.clone())
                    .or_default();
                ui.collapsing("添加池项目", |ui| {
                    random_pool_entry_draft_ui(ui, draft);
                    if ui.button("添加到随机池").clicked() {
                        normalize_random_pool_entry(draft);
                        pool.entries.push(draft.clone());
                        changed = true;
                    }
                });
            },
        );
    }

    if let Some(pool_name) = pool_to_delete {
        manager.random_pools.remove(&pool_name);
        state.random_pool_entry_drafts.remove(&pool_name);
        changed = true;
    }

    changed
}

fn random_pool_entry_draft_ui(ui: &mut Ui, draft: &mut RandomPoolEntry) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        changed |= ui.checkbox(&mut draft.enabled, "启用").changed();
        ui.label("名称");
        changed |= ui
            .add(egui::TextEdit::singleline(&mut draft.item.name).desired_width(120.0))
            .changed();
        changed |= item_quality_combo(ui, &mut draft.item.quality);
        changed |= ui
            .add(
                egui::DragValue::new(&mut draft.weight)
                    .range(0.0..=999_999.0)
                    .speed(0.1)
                    .prefix("权重 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut draft.item.stack)
                    .range(1..=9999)
                    .speed(1)
                    .prefix("物品数量 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut draft.min_count)
                    .range(0..=9999)
                    .speed(1)
                    .prefix("最少出现 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut draft.max_count)
                    .range(0..=9999)
                    .speed(1)
                    .prefix("最多出现 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut draft.item.max_stack)
                    .range(1..=9999)
                    .speed(1)
                    .prefix("最大堆叠 "),
            )
            .changed();
        changed |= equipment_slot_combo(ui, &mut draft.item.equipment_slot);
        changed |= ui
            .add(
                egui::DragValue::new(&mut draft.item.item_level)
                    .range(0..=9999)
                    .speed(1)
                    .prefix("等级 "),
            )
            .changed();
        changed |= ui.checkbox(&mut draft.item.soulbound, "绑定").changed();
    });
    ui.label("文本结果");
    changed |= ui
        .add(
            egui::TextEdit::multiline(&mut draft.result_text)
                .desired_rows(2)
                .desired_width(ui.available_width().min(CHARACTER_FIELD_MAX_WIDTH)),
        )
        .changed();
    ui.label("说明");
    changed |= ui
        .add(
            egui::TextEdit::multiline(&mut draft.item.description)
                .desired_rows(2)
                .desired_width(ui.available_width().min(CHARACTER_FIELD_MAX_WIDTH)),
        )
        .changed();
    changed |= normalize_random_pool_entry(draft);
    changed
}

fn normalize_random_pool_entry(entry: &mut RandomPoolEntry) -> bool {
    let mut changed = normalize_item(&mut entry.item);
    let weight = entry.weight.max(0.0);
    if (entry.weight - weight).abs() > f32::EPSILON {
        entry.weight = weight;
        changed = true;
    }
    let (min_count, max_count) = normalized_random_pool_counts(entry.min_count, entry.max_count);
    if entry.min_count != min_count {
        entry.min_count = min_count;
        changed = true;
    }
    if entry.max_count != max_count {
        entry.max_count = max_count;
        changed = true;
    }
    changed
}

fn random_pool_total_weight(pool: &RandomPool) -> f32 {
    pool.entries
        .iter()
        .filter(|entry| entry.enabled)
        .map(|entry| entry.weight.max(0.0))
        .sum()
}

fn pick_random_pool_entry(pool: &RandomPool) -> Option<RandomPoolEntry> {
    let total = random_pool_total_weight(pool);
    if total <= 0.0 {
        return None;
    }

    let mut roll = rand::rng().random_range(0.0..total);
    for entry in pool.entries.iter().filter(|entry| entry.enabled) {
        let weight = entry.weight.max(0.0);
        if roll < weight {
            return Some(entry.clone());
        }
        roll -= weight;
    }
    None
}

#[cfg(test)]
fn pick_random_pool_item(pool: &RandomPool) -> Option<InventoryItem> {
    pick_random_pool_entry(pool).map(|entry| {
        let mut item = entry.item;
        normalize_item(&mut item);
        item
    })
}

fn random_pool_entry_text_result(entry: &RandomPoolEntry) -> Option<RandomPoolTextResult> {
    let text = entry.result_text.trim();
    if text.is_empty() {
        return None;
    }
    let (min_count, max_count) = normalized_random_pool_counts(entry.min_count, entry.max_count);
    let count = if min_count == max_count {
        min_count
    } else {
        rand::rng().random_range(min_count..=max_count)
    };
    Some(RandomPoolTextResult {
        entry_name: entry.item.name.clone(),
        text: text.to_owned(),
        count,
    })
}

fn random_pool_text_result_label(result: &RandomPoolTextResult) -> String {
    if result.count == 1 {
        result.text.clone()
    } else {
        format!("{} x{}", result.text, result.count)
    }
}

fn random_pool_batch_text_results(pool: &RandomPool, draw_count: u32) -> Vec<RandomPoolTextResult> {
    let draw_count = draw_count.clamp(1, RANDOM_POOL_BATCH_MAX);
    (0..draw_count)
        .filter_map(|_| pick_random_pool_entry(pool))
        .filter_map(|entry| random_pool_entry_text_result(&entry))
        .collect()
}

fn random_pool_checked_results(
    pool: &RandomPool,
    targets: &[NapcatSendTarget],
) -> Vec<RandomPoolCheckedResult> {
    let mut available_targets = targets
        .iter()
        .filter_map(|target| match target {
            NapcatSendTarget::Private(user_id) => Some(user_id.to_string()),
            NapcatSendTarget::Group(_) => None,
        })
        .collect::<Vec<_>>();
    available_targets.sort();
    available_targets.dedup();

    if available_targets.is_empty() {
        return Vec::new();
    }

    let mut results = Vec::new();
    for entry in pool.entries.iter().filter(|entry| entry.enabled) {
        let text = entry.result_text.trim();
        if text.is_empty() {
            continue;
        }
        let (min_count, _) = normalized_random_pool_counts(entry.min_count, entry.max_count);
        let count = min_count as usize;
        if count == 0 || count > available_targets.len() {
            continue;
        }

        for _ in 0..count {
            let target_index = if available_targets.len() == 1 {
                0
            } else {
                rand::rng().random_range(0..available_targets.len())
            };
            results.push(RandomPoolCheckedResult {
                enabled: true,
                target_id: available_targets.remove(target_index),
                text: text.to_owned(),
            });
        }
    }

    results
}

fn random_pool_checked_result_summary(pool: &RandomPool) -> (usize, usize) {
    let total = pool.checked_results.len();
    let enabled = pool
        .checked_results
        .iter()
        .filter(|result| result.enabled && !result.text.trim().is_empty())
        .count();
    (enabled, total)
}

fn random_pool_text_results_message(
    pool_name: &str,
    results: &[RandomPoolTextResult],
) -> Option<String> {
    if results.is_empty() {
        return None;
    }

    let mut lines = Vec::with_capacity(results.len() + 1);
    lines.push(format!("{pool_name}随机结果"));
    for (index, result) in results.iter().enumerate() {
        lines.push(format!(
            "{}. {}",
            index + 1,
            random_pool_text_result_label(result)
        ));
    }
    Some(lines.join("\n"))
}

fn random_pool_send_input_id(pool_name: &str, scope: &str) -> String {
    format!("random-pool:{pool_name}:{scope}")
}

fn random_pool_checked_send_input_id(pool_name: &str, index: usize) -> String {
    format!("random-pool-checked:{pool_name}:{index}")
}

fn queue_random_pool_checked_results(
    pool_name: &str,
    pool: &RandomPool,
    sender: &NapcatIOSender,
    ime: &mut ImeManager,
) -> Result<usize, String> {
    let pending = pool
        .checked_results
        .iter()
        .enumerate()
        .filter(|(_, result)| result.enabled && !result.text.trim().is_empty())
        .map(|(index, result)| {
            let user_id = result.target_id.trim().parse::<u64>().map_err(|_| {
                format!(
                    "随机结果目标不是有效QQ号：{}",
                    result.target_id
                )
            })?;
            Ok((
                index,
                user_id,
                result.text.trim().to_owned(),
            ))
        })
        .collect::<Result<Vec<_>, String>>()?;

    if pending.is_empty() {
        return Err("没有勾选的随机结果".to_owned());
    }

    for (index, user_id, text) in &pending {
        ime.queue_text_send(
            &random_pool_checked_send_input_id(pool_name, *index),
            text,
            sender,
            vec![NapcatSendTarget::Private(*user_id)],
        )?;
    }

    Ok(pending.len())
}

fn unit_pool_settings_ui(
    ui: &mut Ui,
    manager: &mut NapcatMessageManager,
    state: &mut TrpgGroupSettingsState,
    player_targets: &[String],
    mut scene_store: Option<&mut Persistent<VoxelSceneStore>>,
) -> bool {
    let mut changed = false;

    ui.heading("单位池");
    ui.horizontal_wrapped(|ui| {
        ui.label("单位ID");
        ui.add(egui::TextEdit::singleline(&mut state.new_unit_id).desired_width(140.0));
        if ui.button("新建单位").clicked() {
            let unit_id = state.new_unit_id.trim().to_owned();
            if !unit_id.is_empty() {
                let mut unit = state.unit_pool_draft.clone();
                prepare_unit_pool_entry(&unit_id, &mut unit);
                manager.unit_pool.insert(unit_id, unit);
                state.new_unit_id.clear();
                state.unit_pool_draft = UnitPoolEntry::default();
                changed = true;
            }
        }
    });

    ui.collapsing("新单位模板", |ui| {
        changed |= unit_pool_entry_editor_ui(ui, "draft", &mut state.unit_pool_draft);
    });

    if !player_targets.is_empty() {
        if state.unit_pool_source_target.is_empty()
            || !player_targets
                .iter()
                .any(|target_id| target_id == &state.unit_pool_source_target)
        {
            state.unit_pool_source_target = player_targets[0].clone();
        }

        ui.horizontal_wrapped(|ui| {
            egui::ComboBox::from_label("来源角色")
                .selected_text(target_display_name(
                    manager,
                    &state.unit_pool_source_target,
                ))
                .show_ui(ui, |ui| {
                    for target_id in player_targets {
                        ui.selectable_value(
                            &mut state.unit_pool_source_target,
                            target_id.clone(),
                            target_display_name(manager, target_id),
                        );
                    }
                });

            if ui.button("从角色复制").clicked() {
                let source_id = state.unit_pool_source_target.clone();
                let unit_id = if state.new_unit_id.trim().is_empty() {
                    format!("unit-{source_id}")
                } else {
                    state.new_unit_id.trim().to_owned()
                };
                if let Some(character) = manager.player_characters.get(&source_id).cloned() {
                    let mut unit = UnitPoolEntry {
                        label: target_display_name(manager, &source_id),
                        note: "从玩家角色复制".to_owned(),
                        legacy_member_id: None,
                        character,
                    };
                    prepare_unit_pool_entry(&unit_id, &mut unit);
                    manager.unit_pool.insert(unit_id, unit);
                    state.new_unit_id.clear();
                    changed = true;
                }
            }
        });
    } else {
        ui.small("还没有可复制的玩家角色。");
    }

    let mut unit_ids = manager.unit_pool.keys().cloned().collect::<Vec<_>>();
    unit_ids.sort();
    if unit_ids.is_empty() {
        ui.label("还没有单位模板。");
        return changed;
    }

    let mut unit_to_delete = None;
    for unit_id in unit_ids {
        let title = manager
            .unit_pool
            .get(&unit_id)
            .map(|unit| unit_pool_entry_title(&unit_id, unit))
            .unwrap_or_else(|| unit_id.clone());
        ui.collapsing(title, |ui| {
            if let Some(unit) = manager.unit_pool.get_mut(&unit_id) {
                ui.horizontal_wrapped(|ui| {
                    ui.small(format!("ID {unit_id}"));
                    if ui.button("删除单位").clicked() {
                        unit_to_delete = Some(unit_id.clone());
                    }
                    if let Some(store) = scene_store.as_deref_mut() {
                        let image_source = unit.character.image.trim().to_owned();
                        let has_standee = has_unit_template_standee(store, &unit_id);
                        let place_label =
                            if has_standee { "更新场景立绘" } else { "放入场景立绘" };
                        if ui
                            .add_enabled(
                                !image_source.is_empty(),
                                egui::Button::new(place_label),
                            )
                            .on_disabled_hover_text("单位模板还没有立绘")
                            .clicked()
                        {
                            let status = match place_unit_template_standee(
                                &mut *store,
                                &unit_id,
                                &image_source,
                            ) {
                                Ok(scene_changed) => match store.persist() {
                                    Ok(()) => {
                                        if scene_changed {
                                            "已写入场景立绘".to_owned()
                                        } else {
                                            "场景立绘已是最新".to_owned()
                                        }
                                    },
                                    Err(err) => format!("场景立绘保存失败：{err}"),
                                },
                                Err(err) => format!("场景立绘失败：{err}"),
                            };
                            state.unit_pool_scene_status.insert(unit_id.clone(), status);
                        }
                        if has_standee && ui.button("移出场景").clicked() {
                            let removed = remove_unit_template_standee(&mut *store, &unit_id);
                            let status = if removed {
                                match store.persist() {
                                    Ok(()) => "已移出场景立绘".to_owned(),
                                    Err(err) => format!("移出场景保存失败：{err}"),
                                }
                            } else {
                                "场景里没有这个单位立绘".to_owned()
                            };
                            state.unit_pool_scene_status.insert(unit_id.clone(), status);
                        }
                        let has_token = has_unit_template_token(store, &unit_id);
                        let token_label =
                            if has_token { "更新场景标记" } else { "放入场景标记" };
                        if ui.button(token_label).clicked() {
                            let label = if unit.label.trim().is_empty() {
                                unit_id.as_str()
                            } else {
                                unit.label.trim()
                            };
                            let status =
                                match place_unit_template_token(&mut *store, &unit_id, label) {
                                    Ok(scene_changed) => match store.persist() {
                                        Ok(()) => {
                                            if scene_changed {
                                                "已写入场景标记".to_owned()
                                            } else {
                                                "场景标记已是最新".to_owned()
                                            }
                                        },
                                        Err(err) => format!("场景标记保存失败：{err}"),
                                    },
                                    Err(err) => format!("场景标记失败：{err}"),
                                };
                            state.unit_pool_scene_status.insert(unit_id.clone(), status);
                        }
                        if has_token && ui.button("移出标记").clicked() {
                            let removed = remove_unit_template_token(&mut *store, &unit_id);
                            let status = if removed {
                                match store.persist() {
                                    Ok(()) => "已移出场景标记".to_owned(),
                                    Err(err) => format!("移出标记保存失败：{err}"),
                                }
                            } else {
                                "场景里没有这个单位标记".to_owned()
                            };
                            state.unit_pool_scene_status.insert(unit_id.clone(), status);
                        }
                    } else {
                        ui.small("场景未就绪");
                    }
                    if let Some(status) = state.unit_pool_scene_status.get(&unit_id) {
                        ui.small(status);
                    }
                });
                changed |= unit_pool_entry_editor_ui(ui, &unit_id, unit);
            }
        });
    }

    if let Some(unit_id) = unit_to_delete {
        manager.unit_pool.remove(&unit_id);
        if let Some(store) = scene_store.as_deref_mut() {
            let removed_standee = remove_unit_template_standee(&mut *store, &unit_id);
            let removed_token = remove_unit_template_token(&mut *store, &unit_id);
            let removed_scene_objects = removed_standee || removed_token;
            if removed_scene_objects {
                if let Err(err) = store.persist() {
                    state.unit_pool_scene_status.insert(
                        unit_id.clone(),
                        format!("单位已删除；场景对象保存失败：{err}"),
                    );
                }
            }
        }
        changed = true;
    }

    changed
}

fn prepare_unit_pool_entry(unit_id: &str, unit: &mut UnitPoolEntry) {
    let label = unit.label.trim();
    if label.is_empty() {
        unit.label = unit_id.to_owned();
    }

    let label = unit.label.trim();
    if unit.character.nickname.trim().is_empty() {
        unit.character.nickname = label.to_owned();
    }
    if unit.character.name.trim().is_empty() {
        unit.character.name = label.to_owned();
    }
    unit.character.inited = true;
    update_character_from_status(&mut unit.character);
}

fn unit_pool_entry_title(unit_id: &str, unit: &UnitPoolEntry) -> String {
    let label = unit.label.trim();
    if label.is_empty() {
        unit_id.to_owned()
    } else {
        format!("{label} ({unit_id})")
    }
}

fn unit_pool_entry_editor_ui(ui: &mut Ui, unit_id: &str, unit: &mut UnitPoolEntry) -> bool {
    let mut changed = false;

    ui.horizontal_wrapped(|ui| {
        ui.label("显示名");
        changed |= ui
            .add(egui::TextEdit::singleline(&mut unit.label).desired_width(160.0))
            .changed();
        ui.label("旧成员ID");
        let mut legacy_member_id = unit.legacy_member_id.clone().unwrap_or_default();
        if ui
            .add(egui::TextEdit::singleline(&mut legacy_member_id).desired_width(120.0))
            .changed()
        {
            if legacy_member_id.trim().is_empty() {
                unit.legacy_member_id = None;
            } else {
                unit.legacy_member_id = Some(legacy_member_id.trim().to_owned());
            }
            changed = true;
        }
    });
    ui.label("备注");
    changed |= ui
        .add(
            egui::TextEdit::multiline(&mut unit.note)
                .desired_rows(2)
                .desired_width(ui.available_width().min(CHARACTER_FIELD_MAX_WIDTH)),
        )
        .changed();

    changed |= unit_character_template_editor_ui(ui, unit_id, &mut unit.character);
    changed
}

fn unit_character_template_editor_ui(
    ui: &mut Ui,
    unit_id: &str,
    character: &mut PlayerCharacter,
) -> bool {
    let mut changed = false;
    let mut derived_stats_changed = false;

    ui.horizontal_wrapped(|ui| {
        changed |= ui.checkbox(&mut character.inited, "已完成").changed();
        ui.label("角色名");
        changed |= ui
            .add(egui::TextEdit::singleline(&mut character.name).desired_width(120.0))
            .changed();
        ui.label("昵称");
        changed |= ui
            .add(egui::TextEdit::singleline(&mut character.nickname).desired_width(120.0))
            .changed();
    });

    ui.horizontal_wrapped(|ui| {
        let level_response = ui
            .add(
                egui::DragValue::new(&mut character.level)
                    .range(1..=999)
                    .prefix("等级 "),
            )
            .changed();
        changed |= level_response;
        derived_stats_changed |= level_response;
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.exp)
                    .range(0..=999_999)
                    .prefix("经验 "),
            )
            .changed();
        ui.label(format!(
            "/ {}",
            character_next_level_exp(character.level)
        ));
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.speed)
                    .range(0.0..=9999.0)
                    .speed(0.1)
                    .prefix("速度 "),
            )
            .changed();
    });

    ui.horizontal_wrapped(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.hp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("HP "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.max_hp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("/ "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.mp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("MP "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut character.max_mp)
                    .range(0.0..=999_999.0)
                    .speed(0.5)
                    .prefix("/ "),
            )
            .changed();
    });

    let status_changed = unit_character_status_editor_ui(ui, &mut character.status);
    changed |= status_changed;
    derived_stats_changed |= status_changed;

    if derived_stats_changed {
        update_character_from_status(character);
        changed = true;
    }
    if character.hp > character.max_hp {
        character.hp = character.max_hp;
        changed = true;
    }
    if character.mp > character.max_mp {
        character.mp = character.max_mp;
        changed = true;
    }

    character_status_summary_ui(ui, character);
    ui.horizontal_wrapped(|ui| {
        ui.small(format!(
            "技能 {}",
            character.skill_names.len()
        ));
        ui.small(format!(
            "背包 {}",
            character.inventory.items.len()
        ));
        ui.small(format!("模板 {unit_id}"));
    });

    changed
}

fn unit_character_status_editor_ui(ui: &mut Ui, status: &mut CharacterStatus) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.str_)
                    .range(0..=999)
                    .prefix("STR "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.agi)
                    .range(0..=999)
                    .prefix("AGI "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.dex)
                    .range(0..=999)
                    .prefix("DEX "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.vit)
                    .range(0..=999)
                    .prefix("VIT "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.int_)
                    .range(0..=999)
                    .prefix("INT "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.wis)
                    .range(0..=999)
                    .prefix("WIS "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.k)
                    .range(0..=999)
                    .prefix("K "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut status.cha)
                    .range(0..=999)
                    .prefix("CHA "),
            )
            .changed();
    });
    changed
}

fn skill_pool_settings_ui(
    ui: &mut Ui,
    manager: &mut NapcatMessageManager,
    state: &mut TrpgGroupSettingsState,
) -> bool {
    let mut changed = false;
    changed |= manager.sync_skill_pool_from_completed_characters();

    let auto_count = manager
        .skill_pool
        .iter()
        .filter(|entry| entry.source_key().is_some())
        .count();
    let manual_count = manager.skill_pool.len().saturating_sub(auto_count);
    ui.heading("技能池");
    ui.horizontal_wrapped(|ui| {
        ui.small(format!("已兑换技能 {auto_count}"));
        ui.small(format!("手动技能 {manual_count}"));
        if ui.button("刷新已兑换技能").clicked() {
            changed |= manager.sync_skill_pool_from_completed_characters();
        }
    });

    if manager.skill_pool.is_empty() {
        ui.label("还没有技能。完成角色兑换后，技能会自动进入这里。");
    } else {
        let mut remove_manual_index = None;
        egui::ScrollArea::vertical()
            .id_salt("skill_pool_settings")
            .max_height(180.0)
            .show(ui, |ui| {
                egui::Grid::new(ui.next_auto_id())
                    .num_columns(7)
                    .spacing([8.0, 4.0])
                    .striped(true)
                    .show(ui, |ui| {
                        ui.strong("技能");
                        ui.strong("类型");
                        ui.strong("标签");
                        ui.strong("来源");
                        ui.strong("MP");
                        ui.strong("冷却");
                        ui.strong("操作");
                        ui.end_row();

                        for (index, entry) in manager.skill_pool.iter().enumerate() {
                            ui.label(skill_pool_entry_name(entry));
                            ui.small(skill_pool_entry_category_label(entry));
                            ui.small(skill_pool_entry_tags_label(entry));
                            ui.small(entry.source_character_name.as_deref().unwrap_or("手动"));
                            ui.label(format_character_number(entry.mp_cost));
                            ui.label(entry.cooldown_turns.to_string());
                            if entry.source_key().is_none() {
                                if ui.button("-").on_hover_text("移除手动技能").clicked() {
                                    remove_manual_index = Some(index);
                                }
                            } else {
                                ui.small("自动");
                            }
                            ui.end_row();
                        }
                    });
            });
        if let Some(index) = remove_manual_index {
            manager.skill_pool.remove(index);
            changed = true;
        }
    }

    ui.collapsing("添加手动技能", |ui| {
        let draft = &mut state.skill_pool_draft;
        ui.horizontal_wrapped(|ui| {
            ui.label("技能名");
            ui.add(egui::TextEdit::singleline(&mut draft.name).desired_width(140.0));
            ui.add(
                egui::DragValue::new(&mut draft.mp_cost)
                    .range(0.0..=9999.0)
                    .speed(1.0)
                    .prefix("MP "),
            );
            ui.add(
                egui::DragValue::new(&mut draft.cooldown_turns)
                    .range(0..=999)
                    .speed(1)
                    .prefix("冷却 "),
            );
            ui.label("类型");
            ui.add(
                egui::TextEdit::singleline(draft.category.get_or_insert_with(String::new))
                    .desired_width(100.0),
            );
            ui.label("标签");
            let mut tags = draft.tags.join(" ");
            if ui
                .add(egui::TextEdit::singleline(&mut tags).desired_width(140.0))
                .changed()
            {
                draft.tags = tags.split_whitespace().map(str::to_owned).collect();
            }
        });
        ui.label("规则描述");
        ui.add(
            egui::TextEdit::multiline(&mut draft.note)
                .desired_rows(2)
                .desired_width(ui.available_width().min(CHARACTER_FIELD_MAX_WIDTH)),
        );
        if ui.button("加入技能池").clicked() {
            if let Some(mut entry) = normalized_manual_skill_pool_entry(draft) {
                entry.source_character_id = None;
                entry.source_character_name = None;
                entry.source_skill_index = None;
                manager.skill_pool.push(entry);
                state.skill_pool_draft = SkillPoolEntry::default();
                changed = true;
            }
        }
    });

    changed
}

fn normalized_manual_skill_pool_entry(draft: &SkillPoolEntry) -> Option<SkillPoolEntry> {
    let note = draft.note.trim();
    if note.is_empty() {
        return None;
    }
    let name = if draft.name.trim().is_empty() {
        "未命名技能".to_owned()
    } else {
        draft.name.trim().to_owned()
    };
    Some(SkillPoolEntry {
        name,
        note: note.to_owned(),
        mp_cost: draft.mp_cost.max(0.0),
        cooldown_turns: draft.cooldown_turns,
        source_character_id: None,
        source_character_name: None,
        source_skill_index: None,
        tags: draft.tags.clone(),
        category: draft.category.clone(),
        args: draft.args.clone(),
        legacy_buff_count: draft.legacy_buff_count,
        legacy_event_buff_count: draft.legacy_event_buff_count,
        legacy_has_graph: draft.legacy_has_graph,
        legacy_buff_json: draft.legacy_buff_json.clone(),
        legacy_event_buff_json: draft.legacy_event_buff_json.clone(),
        legacy_graph_json: draft.legacy_graph_json.clone(),
        legacy_buff_machine_json: draft.legacy_buff_machine_json.clone(),
        ..Default::default()
    })
}

fn item_pool_settings_ui(
    ui: &mut Ui,
    manager: &mut NapcatMessageManager,
    state: &mut TrpgGroupSettingsState,
    player_targets: &[String],
) -> bool {
    let mut changed = false;
    ui.heading("物品池");
    ui.small("物品池是GM模板库；发给玩家时会复制一份，装备后属性加成立即进入最终数值。");

    if !player_targets.is_empty() {
        if !player_targets.contains(&state.item_pool_award_target) {
            state.item_pool_award_target = player_targets[0].clone();
        }
        egui::ComboBox::from_label("发放给玩家")
            .selected_text(target_display_name(
                manager,
                &state.item_pool_award_target,
            ))
            .show_ui(ui, |ui| {
                for target_id in player_targets {
                    ui.selectable_value(
                        &mut state.item_pool_award_target,
                        target_id.clone(),
                        target_display_name(manager, target_id),
                    );
                }
            });
    }

    let mut remove_index = None;
    let mut award_index = None;
    for (index, item) in manager.item_pool.iter_mut().enumerate() {
        ui.push_id(("item_pool_entry", index), |ui| {
            ui.collapsing(
                format!(
                    "{} · {} · {}项加成",
                    item_display_name(item),
                    equipment_slot_label(item.equipment_slot),
                    item.stat_effects.len()
                ),
                |ui| {
                    changed |= inventory_item_definition_ui(ui, item);
                    ui.horizontal(|ui| {
                        if ui
                            .add_enabled(
                                !player_targets.is_empty(),
                                egui::Button::new("发给玩家"),
                            )
                            .clicked()
                        {
                            award_index = Some(index);
                        }
                        if ui.button("从物品池移除").clicked() {
                            remove_index = Some(index);
                        }
                    });
                },
            );
        });
    }
    if let Some(index) = award_index {
        if let Some(item) = manager.item_pool.get(index).cloned() {
            let character = manager
                .player_characters
                .entry(state.item_pool_award_target.clone())
                .or_default();
            add_item_to_inventory(&mut character.inventory, item);
            changed = true;
        }
    }
    if let Some(index) = remove_index {
        manager.item_pool.remove(index);
        changed = true;
    }

    ui.separator();
    ui.collapsing("添加物品模板", |ui| {
        changed |= inventory_item_definition_ui(ui, &mut state.item_pool_draft);
        if ui.button("加入物品池").clicked() {
            let mut item = state.item_pool_draft.clone();
            if !item.name.trim().is_empty() {
                item.name = item.name.trim().to_owned();
                normalize_item(&mut item);
                manager.item_pool.push(item);
                state.item_pool_draft = InventoryItem::default();
                changed = true;
            }
        }
    });
    changed
}

fn inventory_item_definition_ui(ui: &mut Ui, item: &mut InventoryItem) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        ui.label("物品");
        changed |= ui
            .add(egui::TextEdit::singleline(&mut item.name).desired_width(150.0))
            .changed();
        changed |= item_quality_combo(ui, &mut item.quality);
        changed |= equipment_slot_combo(ui, &mut item.equipment_slot);
        changed |= ui
            .add(
                egui::DragValue::new(&mut item.item_level)
                    .range(0..=9999)
                    .prefix("等级 "),
            )
            .changed();
        changed |= ui.checkbox(&mut item.soulbound, "绑定").changed();
    });
    ui.horizontal_wrapped(|ui| {
        changed |= ui
            .add(
                egui::DragValue::new(&mut item.stack)
                    .range(1..=9999)
                    .prefix("数量 "),
            )
            .changed();
        changed |= ui
            .add(
                egui::DragValue::new(&mut item.max_stack)
                    .range(1..=9999)
                    .prefix("堆叠上限 "),
            )
            .changed();
    });
    ui.label("描述");
    changed |= ui
        .add(
            egui::TextEdit::multiline(&mut item.description)
                .desired_rows(2)
                .desired_width(ui.available_width().min(CHARACTER_FIELD_MAX_WIDTH)),
        )
        .changed();
    ui.collapsing(
        format!(
            "装备属性加成 ({})",
            item.stat_effects.len()
        ),
        |ui| {
            changed |= item_stat_effects_editor_ui(ui, &mut item.stat_effects);
        },
    );
    changed
}

fn item_stat_effects_editor_ui(ui: &mut Ui, effects: &mut Vec<BuffEffect>) -> bool {
    let before = effects.clone();
    let mut remove_index = None;
    for (index, effect) in effects.iter_mut().enumerate() {
        ui.push_id(("item_stat_effect", index), |ui| {
            ui.horizontal_wrapped(|ui| {
                buff_field_combo(ui, &mut effect.field);
                buff_value_ui(ui, &mut effect.value);
                if ui.button("-").on_hover_text("移除属性加成").clicked() {
                    remove_index = Some(index);
                }
            });
        });
    }
    if let Some(index) = remove_index {
        effects.remove(index);
    }
    if ui.button("+ 属性加成").clicked() {
        effects.push(BuffEffect {
            field: BuffField::Speed,
            value: BuffValue::Add(0.0),
        });
    }
    *effects != before
}

fn skill_pool_entry_name(entry: &SkillPoolEntry) -> String {
    if entry.name.trim().is_empty() {
        "未命名技能".to_owned()
    } else {
        entry.name.trim().to_owned()
    }
}

fn skill_pool_entry_category_label(entry: &SkillPoolEntry) -> String {
    entry
        .category
        .as_deref()
        .filter(|category| !category.trim().is_empty())
        .unwrap_or("-")
        .to_owned()
}

fn skill_pool_entry_tags_label(entry: &SkillPoolEntry) -> String {
    if entry.tags.is_empty() {
        "-".to_owned()
    } else {
        entry.tags.join(" ")
    }
}

fn legacy_json_size_label(label: &str, value: &Option<String>) -> Option<String> {
    value
        .as_deref()
        .filter(|json| !json.trim().is_empty())
        .map(|json| format!("{label}原文 {}字节", json.len()))
}

fn skill_pool_entry_legacy_label(entry: &SkillPoolEntry) -> Option<String> {
    let mut parts = Vec::new();
    if let Some(id) = entry
        .legacy_pool_id
        .as_deref()
        .filter(|id| !id.trim().is_empty())
    {
        parts.push(format!("旧ID {id}"));
    }
    if !entry.args.is_empty() {
        let args = entry
            .args
            .iter()
            .map(|arg| {
                let name =
                    if arg.name.trim().is_empty() { "未命名变量" } else { arg.name.trim() };
                if arg.kind.trim().is_empty() {
                    name.to_owned()
                } else {
                    format!("{name}:{}", arg.kind.trim())
                }
            })
            .collect::<Vec<_>>()
            .join(", ");
        parts.push(format!("变量 {args}"));
    }
    if entry.legacy_buff_count > 0 {
        parts.push(format!(
            "旧BUFF {}",
            entry.legacy_buff_count
        ));
    }
    if entry.legacy_event_buff_count > 0 {
        parts.push(format!(
            "旧事件BUFF {}",
            entry.legacy_event_buff_count
        ));
    }
    if entry.legacy_has_graph {
        parts.push("含旧蓝图".to_owned());
    }
    if let Some(label) = legacy_json_size_label("旧BUFF", &entry.legacy_buff_json) {
        parts.push(label);
    }
    if let Some(label) = legacy_json_size_label(
        "旧事件BUFF",
        &entry.legacy_event_buff_json,
    ) {
        parts.push(label);
    }
    if let Some(label) = legacy_json_size_label("旧蓝图", &entry.legacy_graph_json) {
        parts.push(label);
    }
    if let Some(label) = legacy_json_size_label(
        "旧buff机",
        &entry.legacy_buff_machine_json,
    ) {
        parts.push(label);
    }
    (!parts.is_empty()).then(|| parts.join("；"))
}

fn ensure_import_export_paths(state: &mut TrpgGroupSettingsState) {
    if state.export_path.trim().is_empty() {
        state.export_path = NAPCAT_EXPORT_DEFAULT_PATH.to_owned();
    }
    if state.pc_export_path.trim().is_empty() {
        state.pc_export_path = NAPCAT_PC_EXPORT_DEFAULT_PATH.to_owned();
    }
    if state.chat_list_export_path.trim().is_empty() {
        state.chat_list_export_path = NAPCAT_CHAT_LIST_EXPORT_DEFAULT_PATH.to_owned();
    }
    if state.unit_pool_export_path.trim().is_empty() {
        state.unit_pool_export_path = NAPCAT_UNIT_POOL_EXPORT_DEFAULT_PATH.to_owned();
    }
    if state.moonberry_legacy_import_path.trim().is_empty() {
        state.moonberry_legacy_import_path = NAPCAT_MOONBERRY_LEGACY_IMPORT_DEFAULT_PATH.to_owned();
    }
    if state.deepseek_summary_export_path.trim().is_empty() {
        state.deepseek_summary_export_path = DEEPSEEK_SUMMARY_EXPORT_DEFAULT_PATH.to_owned();
    }
    if state.voxel_scene_export_path.trim().is_empty() {
        state.voxel_scene_export_path = VOXEL_SCENE_EXPORT_DEFAULT_PATH.to_owned();
    }
    if state.battle_round_export_path.trim().is_empty() {
        state.battle_round_export_path = BATTLE_ROUND_EXPORT_DEFAULT_PATH.to_owned();
    }
    if state.import_path.trim().is_empty() {
        state.import_path = state.export_path.clone();
    }
}

fn napcat_import_export_ui(
    ui: &mut Ui,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    deepseek_manager: &mut ResMut<Persistent<DeepseekManager>>,
    mut scene_store: Option<&mut Persistent<VoxelSceneStore>>,
    mut scene_runtime: Option<&mut VoxelMapRuntimeState>,
    mut battle_store: Option<&mut Persistent<BattleRoundStore>>,
    state: &mut TrpgGroupSettingsState,
) {
    ensure_import_export_paths(state);

    ui.collapsing("导入/导出", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.label(format!(
                "NapCat格式版本 {} / DeepSeek总结版本 {} / 场景版本 {} / 战斗轮版本 {}",
                NAPCAT_MANAGER_EXPORT_VERSION,
                DEEPSEEK_SUMMARY_EXPORT_VERSION,
                VOXEL_SCENE_EXPORT_VERSION,
                BATTLE_ROUND_EXPORT_VERSION
            ));
            if !state.import_export_status.trim().is_empty() {
                ui.small(state.import_export_status.as_str());
            }
        });

        ui.horizontal(|ui| {
            ui.label("导出路径");
            ui.text_edit_singleline(&mut state.export_path);
            if ui.button("导出").clicked() {
                match write_napcat_manager_export(manager, &state.export_path) {
                    Ok(()) => {
                        state.import_export_status = format!("已导出到 {}", state.export_path);
                    },
                    Err(err) => {
                        state.import_export_status = format!("导出失败：{err}");
                    },
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("PC文件");
            ui.text_edit_singleline(&mut state.pc_export_path);
            if ui.button("导出PC").clicked() {
                match write_text_export(
                    &state.pc_export_path,
                    manager.to_player_characters_export_json(),
                ) {
                    Ok(()) => {
                        state.import_export_status = format!("已导出PC到 {}", state.pc_export_path);
                    },
                    Err(err) => {
                        state.import_export_status = format!("PC导出失败：{err}");
                    },
                }
            }
            if ui.button("导入PC").clicked() {
                match read_text_import(&state.pc_export_path)
                    .and_then(|text| manager.merge_player_characters_export_json(&text))
                {
                    Ok(count) => match manager.persist() {
                        Ok(()) => {
                            state.import_export_status = format!("已导入{}个PC", count);
                        },
                        Err(err) => {
                            state.import_export_status = format!("PC导入保存失败：{err}");
                        },
                    },
                    Err(err) => {
                        state.import_export_status = format!("PC导入失败：{err}");
                    },
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("聊天列表文件");
            ui.text_edit_singleline(&mut state.chat_list_export_path);
            if ui.button("导出聊天列表").clicked() {
                match write_text_export(
                    &state.chat_list_export_path,
                    manager.to_chat_list_export_json(),
                ) {
                    Ok(()) => {
                        state.import_export_status = format!(
                            "已导出聊天列表到 {}",
                            state.chat_list_export_path
                        );
                    },
                    Err(err) => {
                        state.import_export_status = format!("聊天列表导出失败：{err}");
                    },
                }
            }
            if ui.button("导入聊天列表").clicked() {
                match read_text_import(&state.chat_list_export_path)
                    .and_then(|text| manager.merge_chat_list_export_json(&text))
                {
                    Ok(count) => match manager.persist() {
                        Ok(()) => {
                            state.import_export_status = format!("已导入{}个聊天目标", count);
                        },
                        Err(err) => {
                            state.import_export_status = format!("聊天列表导入保存失败：{err}");
                        },
                    },
                    Err(err) => {
                        state.import_export_status = format!("聊天列表导入失败：{err}");
                    },
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("单位池文件");
            ui.text_edit_singleline(&mut state.unit_pool_export_path);
            if ui.button("导出单位池").clicked() {
                match write_text_export(
                    &state.unit_pool_export_path,
                    manager.to_unit_pool_export_json(),
                ) {
                    Ok(()) => {
                        state.import_export_status = format!(
                            "已导出单位池到 {}",
                            state.unit_pool_export_path
                        );
                    },
                    Err(err) => {
                        state.import_export_status = format!("单位池导出失败：{err}");
                    },
                }
            }
            if ui.button("导入单位池").clicked() {
                match read_text_import(&state.unit_pool_export_path)
                    .and_then(|text| manager.merge_unit_pool_export_json(&text))
                {
                    Ok(count) => match manager.persist() {
                        Ok(()) => {
                            state.import_export_status = format!("已导入{}个单位模板", count);
                        },
                        Err(err) => {
                            state.import_export_status = format!("单位池导入保存失败：{err}");
                        },
                    },
                    Err(err) => {
                        state.import_export_status = format!("单位池导入失败：{err}");
                    },
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("月莓旧JSON");
            ui.text_edit_singleline(&mut state.moonberry_legacy_import_path);
            if ui.button("导入月莓旧JSON").clicked() {
                match read_text_import(&state.moonberry_legacy_import_path)
                    .and_then(|text| manager.merge_moonberry_legacy_json(&text))
                {
                    Ok(summary) => match manager.persist() {
                        Ok(()) => {
                            state.import_export_status = format!(
                                "已导入月莓旧JSON：{}个团，{}个PC，{}个聊天目标，{}条消息，{}个技能池，{}个单位模板，{}个随机池，{}个旧频道，{}个旧世界，{}个虚拟讨论组，{}个旧发送窗",
                                summary.groups,
                                summary.players,
                                summary.chat_targets,
                                summary.messages,
                                summary.skill_pools,
                                summary.unit_templates,
                                summary.random_pools,
                                summary.legacy_teams,
                                summary.legacy_worlds,
                                summary.legacy_chat_areas,
                                summary.legacy_send_panes
                            );
                        },
                        Err(err) => {
                            state.import_export_status = format!("月莓旧JSON导入保存失败：{err}");
                        },
                    },
                    Err(err) => {
                        state.import_export_status = format!("月莓旧JSON导入失败：{err}");
                    },
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("DeepSeek总结文件");
            ui.text_edit_singleline(&mut state.deepseek_summary_export_path);
            if ui.button("导出总结").clicked() {
                match write_text_export(
                    &state.deepseek_summary_export_path,
                    deepseek_manager.to_summary_export_json(),
                ) {
                    Ok(()) => {
                        state.import_export_status = format!(
                            "已导出DeepSeek总结到 {}",
                            state.deepseek_summary_export_path
                        );
                    },
                    Err(err) => {
                        state.import_export_status = format!("DeepSeek总结导出失败：{err}");
                    },
                }
            }
            if ui.button("导入总结").clicked() {
                match read_text_import(&state.deepseek_summary_export_path)
                    .and_then(|text| deepseek_manager.merge_summary_export_json(&text))
                {
                    Ok(count) => match deepseek_manager.persist() {
                        Ok(()) => {
                            state.import_export_status = format!("已导入{}个DeepSeek总结", count);
                        },
                        Err(err) => {
                            state.import_export_status = format!("DeepSeek总结导入保存失败：{err}");
                        },
                    },
                    Err(err) => {
                        state.import_export_status = format!("DeepSeek总结导入失败：{err}");
                    },
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("体素场景文件");
            ui.text_edit_singleline(&mut state.voxel_scene_export_path);
            if ui.button("导出场景").clicked() {
                let result = scene_store
                    .as_deref()
                    .ok_or_else(|| "场景存储未就绪".to_owned())
                    .and_then(|store| {
                        write_text_export(
                            &state.voxel_scene_export_path,
                            store.to_export_json(),
                        )
                    });
                match result {
                    Ok(()) => {
                        state.import_export_status = format!(
                            "已导出体素场景到 {}",
                            state.voxel_scene_export_path
                        );
                    },
                    Err(err) => {
                        state.import_export_status = format!("体素场景导出失败：{err}");
                    },
                }
            }
            if ui.button("导入场景").clicked() {
                let result = scene_store
                    .as_deref_mut()
                    .ok_or_else(|| "场景存储未就绪".to_owned())
                    .and_then(|store| {
                        let text = read_text_import(&state.voxel_scene_export_path)?;
                        let count = store.merge_export_json(&text)?;
                        store.persist().map_err(|err| err.to_string())?;
                        if let Some(runtime) = scene_runtime.as_deref_mut() {
                            runtime.request_reload();
                        }
                        Ok(count)
                    });
                match result {
                    Ok(count) => {
                        state.import_export_status = format!("已导入{}张体素地图", count);
                    },
                    Err(err) => {
                        state.import_export_status = format!("体素场景导入失败：{err}");
                    },
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("战斗轮文件");
            ui.text_edit_singleline(&mut state.battle_round_export_path);
            if ui.button("导出战斗轮").clicked() {
                let result = battle_store
                    .as_deref()
                    .ok_or_else(|| "战斗轮存储未就绪".to_owned())
                    .and_then(|store| {
                        write_text_export(
                            &state.battle_round_export_path,
                            store.to_export_json(),
                        )
                    });
                match result {
                    Ok(()) => {
                        state.import_export_status = format!(
                            "已导出战斗轮到 {}",
                            state.battle_round_export_path
                        );
                    },
                    Err(err) => {
                        state.import_export_status = format!("战斗轮导出失败：{err}");
                    },
                }
            }
            if ui.button("导入战斗轮（替换）").clicked() {
                let result = battle_store
                    .as_deref_mut()
                    .ok_or_else(|| "战斗轮存储未就绪".to_owned())
                    .and_then(|store| {
                        let text = read_text_import(&state.battle_round_export_path)?;
                        let imported = BattleRoundStore::from_export_json(&text)?;
                        let count = imported.encounters.len();
                        store.set(imported).map_err(|err| err.to_string())?;
                        Ok(count)
                    });
                match result {
                    Ok(count) => {
                        state.import_export_status = format!("已替换导入{count}个战斗轮");
                    },
                    Err(err) => {
                        state.import_export_status = format!("战斗轮导入失败：{err}");
                    },
                }
            }
        });

        ui.horizontal(|ui| {
            ui.label("导入路径");
            ui.text_edit_singleline(&mut state.import_path);
            if ui.button("导入").clicked() {
                match read_napcat_manager_export(&state.import_path) {
                    Ok(imported) => match manager.set(imported) {
                        Ok(()) => {
                            let quarantine_result = battle_store.as_deref_mut().map(|store| {
                                let total = store.encounters.len();
                                store.quarantine_manager_sync();
                                store
                                    .persist()
                                    .map(|()| total)
                                    .map_err(|err| err.to_string())
                            });
                            state.import_export_status = match quarantine_result {
                                Some(Ok(count)) if count > 0 => format!(
                                    "已从 {} 导入；已隔离{count}个旧战斗轮，需在战斗面板确认连接或导入配套战斗备份",
                                    state.import_path
                                ),
                                Some(Ok(_)) | None => {
                                    format!("已从 {} 导入", state.import_path)
                                },
                                Some(Err(err)) => format!(
                                    "已从 {} 导入；旧战斗轮已在本次运行中隔离，但隔离状态保存失败：{err}",
                                    state.import_path
                                ),
                            };
                        },
                        Err(err) => {
                            state.import_export_status = format!("导入保存失败：{err}");
                        },
                    },
                    Err(err) => {
                        state.import_export_status = format!("导入失败：{err}");
                    },
                }
            }
        });
    });
}

fn write_napcat_manager_export(manager: &NapcatMessageManager, path: &str) -> Result<(), String> {
    write_text_export(path, manager.to_export_json())
}

fn write_text_export(path: &str, text: Result<String, String>) -> Result<(), String> {
    let path = Path::new(path.trim());
    if path.as_os_str().is_empty() {
        return Err("路径不能为空".to_owned());
    }
    if let Some(parent) = path
        .parent()
        .filter(|parent| !parent.as_os_str().is_empty())
    {
        fs::create_dir_all(parent).map_err(|err| err.to_string())?;
    }
    fs::write(path, text?).map_err(|err| err.to_string())
}

fn read_napcat_manager_export(path: &str) -> Result<NapcatMessageManager, String> {
    let text = read_text_import(path)?;
    NapcatMessageManager::from_export_json(&text)
}

fn read_text_import(path: &str) -> Result<String, String> {
    let path = Path::new(path.trim());
    if path.as_os_str().is_empty() {
        return Err("路径不能为空".to_owned());
    }
    fs::read_to_string(path).map_err(|err| err.to_string())
}

fn trpg_basic_config_ui(ui: &mut Ui, config: &mut TrpgBasicConfig) -> bool {
    let mut changed = false;
    ui.horizontal_wrapped(|ui| {
        changed |= f32_config_drag(
            ui,
            "基础HP",
            &mut config.base_max_hp,
            0.0..=9999.0,
            0.5,
        );
        changed |= f32_config_drag(
            ui,
            "等级HP",
            &mut config.lv_max_hp,
            0.0..=9999.0,
            0.5,
        );
        changed |= f32_config_drag(
            ui,
            "力量HP",
            &mut config.str_max_hp,
            0.0..=9999.0,
            0.5,
        );
        changed |= f32_config_drag(
            ui,
            "体质HP",
            &mut config.vit_max_hp,
            0.0..=9999.0,
            0.5,
        );
        changed |= f32_config_drag(
            ui,
            "体质回复",
            &mut config.vit_hp_reg,
            -999.0..=999.0,
            0.1,
        );
    });
    ui.horizontal_wrapped(|ui| {
        changed |= f32_config_drag(
            ui,
            "智力MP",
            &mut config.int_max_mp,
            0.0..=9999.0,
            0.5,
        );
        changed |= f32_config_drag(
            ui,
            "智慧MP",
            &mut config.wis_max_mp,
            0.0..=9999.0,
            0.5,
        );
        changed |= f32_config_drag(
            ui,
            "智慧回蓝",
            &mut config.wis_mp_reg,
            -999.0..=999.0,
            0.1,
        );
        changed |= f32_config_drag(
            ui,
            "基础速度",
            &mut config.basic_speed,
            0.0..=999.0,
            0.1,
        );
        changed |= f32_config_drag(
            ui,
            "力量速度",
            &mut config.str_speed,
            -999.0..=999.0,
            0.1,
        );
        changed |= f32_config_drag(
            ui,
            "敏捷速度",
            &mut config.agi_speed,
            -999.0..=999.0,
            0.1,
        );
        changed |= f32_config_drag(
            ui,
            "灵巧速度",
            &mut config.dex_speed,
            -999.0..=999.0,
            0.1,
        );
    });
    ui.collapsing("伤害与治疗系数", |ui| {
        ui.horizontal_wrapped(|ui| {
            changed |= f32_config_drag(
                ui,
                "力量伤害",
                &mut config.str_damage_bonus,
                -99.0..=99.0,
                0.001,
            );
            changed |= f32_config_drag(
                ui,
                "智力伤害",
                &mut config.int_damage_bonus,
                -99.0..=99.0,
                0.001,
            );
            changed |= f32_config_drag(
                ui,
                "敏捷伤害",
                &mut config.agi_damage_bonus,
                -99.0..=99.0,
                0.001,
            );
            changed |= f32_config_drag(
                ui,
                "灵巧伤害",
                &mut config.dex_damage_bonus,
                -99.0..=99.0,
                0.001,
            );
            changed |= f32_config_drag(
                ui,
                "灵巧远程",
                &mut config.dex_range_damage_bonus,
                -99.0..=99.0,
                0.001,
            );
            changed |= f32_config_drag(
                ui,
                "智力治疗",
                &mut config.int_heal_bonus,
                -99.0..=99.0,
                0.001,
            );
            changed |= f32_config_drag(
                ui,
                "智慧治疗",
                &mut config.wis_heal_bonus,
                -99.0..=99.0,
                0.001,
            );
        });
    });
    ui.horizontal_wrapped(|ui| {
        changed |= f32_config_drag(
            ui,
            "升级经验",
            &mut config.exp_gain_per_level,
            0.0..=9999.0,
            0.1,
        );
        changed |= f32_config_drag(
            ui,
            "PVP经验",
            &mut config.exp_gain_per_level_pvp,
            0.0..=9999.0,
            0.01,
        );
        if ui.button("恢复默认公式").clicked() {
            *config = TrpgBasicConfig::default();
            changed = true;
        }
    });
    changed
}

fn f32_config_drag(
    ui: &mut Ui,
    label: &str,
    value: &mut f32,
    range: std::ops::RangeInclusive<f32>,
    speed: f64,
) -> bool {
    ui.add(
        egui::DragValue::new(value)
            .range(range)
            .speed(speed)
            .prefix(format!("{label} ")),
    )
    .changed()
}

fn legacy_member_preview(manager: &NapcatMessageManager, members: &[String]) -> String {
    if members.is_empty() {
        return "无成员".to_owned();
    }
    let mut labels = members
        .iter()
        .take(6)
        .map(|member_id| target_display_name(manager, member_id))
        .collect::<Vec<_>>();
    if members.len() > labels.len() {
        labels.push(format!(
            "另{}人",
            members.len() - labels.len()
        ));
    }
    labels.join("、")
}

fn unit_template_scene_label(manager: &NapcatMessageManager, unit_id: &str) -> String {
    manager
        .unit_pool
        .get(unit_id)
        .map(|unit| unit.label.trim())
        .filter(|label| !label.is_empty())
        .unwrap_or(unit_id)
        .to_owned()
}

fn place_legacy_world_unit_tokens_ui(
    store: &mut Persistent<VoxelSceneStore>,
    manager: &NapcatMessageManager,
    group_name: &str,
    world_id: &str,
    world_name: &str,
    unit_ids: &[String],
    visible: bool,
) -> String {
    if unit_ids.is_empty() {
        return "没有匹配的单位模板".to_owned();
    }

    let mut changed = false;
    for unit_id in unit_ids {
        let label = unit_template_scene_label(manager, unit_id);
        match place_legacy_world_unit_token(
            &mut *store,
            group_name,
            world_id,
            world_name,
            unit_id,
            &label,
            visible,
        ) {
            Ok(scene_changed) => changed |= scene_changed,
            Err(err) => return format!("世界NPC标记失败：{err}"),
        }
    }
    let removed = prune_legacy_world_unit_tokens(
        &mut *store,
        group_name,
        world_id,
        unit_ids,
    );
    changed |= removed > 0;

    if changed {
        match store.persist() {
            Ok(()) => {
                if removed > 0 {
                    format!(
                        "已同步世界NPC标记 {} 个，移除旧标记 {removed} 个",
                        unit_ids.len()
                    )
                } else {
                    format!(
                        "已写入世界NPC标记 {} 个",
                        unit_ids.len()
                    )
                }
            },
            Err(err) => format!("世界NPC标记保存失败：{err}"),
        }
    } else {
        format!(
            "世界NPC标记已是最新（{}个）",
            unit_ids.len()
        )
    }
}

fn remove_legacy_world_unit_tokens_ui(
    store: &mut Persistent<VoxelSceneStore>,
    group_name: &str,
    world_id: &str,
) -> String {
    let removed = remove_legacy_world_unit_tokens(&mut *store, group_name, world_id);
    if removed == 0 {
        return "场景里没有这个旧世界NPC标记".to_owned();
    }
    match store.persist() {
        Ok(()) => format!("已移出世界NPC标记 {removed} 个"),
        Err(err) => format!("移出世界NPC标记保存失败：{err}"),
    }
}

fn place_legacy_area_unit_tokens_ui(
    store: &mut Persistent<VoxelSceneStore>,
    manager: &NapcatMessageManager,
    group_name: &str,
    world_id: &str,
    area_id: &str,
    area_name: &str,
    unit_ids: &[String],
    x: f32,
    y: f32,
    width: f32,
    height: f32,
    visible: bool,
) -> String {
    if unit_ids.is_empty() {
        return "没有匹配的单位模板".to_owned();
    }

    let mut changed = false;
    for (index, unit_id) in unit_ids.iter().enumerate() {
        let label = unit_template_scene_label(manager, unit_id);
        match place_legacy_area_unit_token(
            &mut *store,
            group_name,
            world_id,
            area_id,
            area_name,
            unit_id,
            &label,
            x,
            y,
            width,
            height,
            visible,
            index,
        ) {
            Ok(scene_changed) => changed |= scene_changed,
            Err(err) => return format!("区域单位标记失败：{err}"),
        }
    }
    let removed = prune_legacy_area_unit_tokens(
        &mut *store,
        group_name,
        world_id,
        area_id,
        unit_ids,
    );
    changed |= removed > 0;

    if changed {
        match store.persist() {
            Ok(()) => {
                if removed > 0 {
                    format!(
                        "已同步区域单位标记 {} 个，移除旧标记 {removed} 个",
                        unit_ids.len()
                    )
                } else {
                    format!(
                        "已写入区域单位标记 {} 个",
                        unit_ids.len()
                    )
                }
            },
            Err(err) => format!("区域单位标记保存失败：{err}"),
        }
    } else {
        format!(
            "区域单位标记已是最新（{}个）",
            unit_ids.len()
        )
    }
}

fn remove_legacy_area_unit_tokens_ui(
    store: &mut Persistent<VoxelSceneStore>,
    group_name: &str,
    world_id: &str,
    area_id: &str,
) -> String {
    let removed = remove_legacy_area_unit_tokens(
        &mut *store,
        group_name,
        world_id,
        area_id,
    );
    if removed == 0 {
        return "场景里没有这个旧区域单位标记".to_owned();
    }
    match store.persist() {
        Ok(()) => format!("已移出区域单位标记 {removed} 个"),
        Err(err) => format!("移出区域单位标记保存失败：{err}"),
    }
}

fn legacy_team_chat_sender_label(
    manager: &NapcatMessageManager,
    sender_id: &str,
    sender_name: &str,
) -> String {
    let sender_id = sender_id.trim();
    let sender_name = sender_name.trim();
    if !sender_id.is_empty() {
        let display_name = target_display_name(manager, sender_id);
        if !display_name.trim().is_empty() && display_name.trim() != sender_id {
            return display_name;
        }
    }
    if !sender_name.is_empty() {
        return sender_name.to_owned();
    }
    if !sender_id.is_empty() {
        return sender_id.to_owned();
    }
    "未知发送者".to_owned()
}

#[derive(Clone)]
enum LegacyPartyPromotion {
    Team(String),
    ChatArea(String),
}

#[derive(Clone)]
enum LegacyGroupSurfaceAction {
    Promote(LegacyPartyPromotion),
    AppendTeamChat {
        team_id: String,
        message: TrpgLegacyTeamChatMessage,
    },
    UpdateTeamChat {
        team_id: String,
        message_index: usize,
        text: String,
    },
    RemoveTeamChat {
        team_id: String,
        message_index: usize,
    },
    AddSendPane,
    RemoveSendPane {
        pane_key: String,
    },
    ClearSendPaneTargets {
        pane_key: String,
    },
    SetSendPaneTarget {
        pane_key: String,
        target_id: String,
        selected: bool,
    },
}

fn apply_legacy_group_surface_action(
    manager: &mut NapcatMessageManager,
    state: &mut TrpgGroupSettingsState,
    group_name: &str,
    action: LegacyGroupSurfaceAction,
) -> bool {
    let Some(group) = manager.trpg_groups.get_mut(group_name) else {
        return false;
    };
    match action {
        LegacyGroupSurfaceAction::Promote(promotion) => match promotion {
            LegacyPartyPromotion::Team(team_id) => group.promote_legacy_team_to_party(&team_id),
            LegacyPartyPromotion::ChatArea(area_id) => {
                group.promote_legacy_chat_area_to_party(&area_id)
            },
        },
        LegacyGroupSurfaceAction::AppendTeamChat { team_id, message } => {
            group.append_legacy_team_chat_message(&team_id, message)
        },
        LegacyGroupSurfaceAction::UpdateTeamChat {
            team_id,
            message_index,
            text,
        } => {
            let changed = group.update_legacy_team_chat_message(&team_id, message_index, &text);
            if changed {
                state
                    .legacy_team_chat_edit_drafts
                    .remove(&legacy_team_chat_edit_id(
                        group_name,
                        &team_id,
                        message_index,
                    ));
            }
            state.legacy_team_chat_status.insert(
                legacy_team_chat_input_id(group_name, &team_id),
                if changed {
                    "已更新本地旧频道消息".to_owned()
                } else {
                    "没有可更新的本地旧频道消息".to_owned()
                },
            );
            changed
        },
        LegacyGroupSurfaceAction::RemoveTeamChat {
            team_id,
            message_index,
        } => {
            let changed = group.remove_legacy_team_chat_message(&team_id, message_index);
            if changed {
                let draft_prefix = legacy_team_chat_edit_prefix(group_name, &team_id);
                state
                    .legacy_team_chat_edit_drafts
                    .retain(|key, _| !key.starts_with(&draft_prefix));
            }
            state.legacy_team_chat_status.insert(
                legacy_team_chat_input_id(group_name, &team_id),
                if changed {
                    "已删除本地旧频道消息".to_owned()
                } else {
                    "没有可删除的本地旧频道消息".to_owned()
                },
            );
            changed
        },
        LegacyGroupSurfaceAction::AddSendPane => group.add_legacy_send_pane("多选发送").is_some(),
        LegacyGroupSurfaceAction::RemoveSendPane { pane_key } => {
            group.remove_legacy_send_pane(&pane_key)
        },
        LegacyGroupSurfaceAction::ClearSendPaneTargets { pane_key } => {
            group.clear_legacy_send_pane_targets(&pane_key)
        },
        LegacyGroupSurfaceAction::SetSendPaneTarget {
            pane_key,
            target_id,
            selected,
        } => group.set_legacy_send_pane_target(&pane_key, &target_id, selected),
    }
}

fn legacy_pane_has_target(pane: &TrpgLegacySendPane, target_id: &str) -> bool {
    pane.targets
        .iter()
        .any(|target| target.trim() == target_id.trim())
}

fn legacy_send_pane_target_checkbox(
    ui: &mut Ui,
    pane: &TrpgLegacySendPane,
    target_id: &str,
    label: String,
    enabled: bool,
    hover_text: &str,
) -> Option<LegacyGroupSurfaceAction> {
    let mut selected = legacy_pane_has_target(pane, target_id);
    let response = ui.add_enabled(
        enabled,
        egui::Checkbox::new(&mut selected, label),
    );
    let changed = response.changed();
    response.on_hover_text(hover_text);
    changed.then(
        || LegacyGroupSurfaceAction::SetSendPaneTarget {
            pane_key: pane.key.clone(),
            target_id: target_id.to_owned(),
            selected,
        },
    )
}

fn legacy_send_pane_target_editor_ui(
    ui: &mut Ui,
    manager: &NapcatMessageManager,
    group: &TrpgGroup,
    pane: &TrpgLegacySendPane,
) -> Option<LegacyGroupSurfaceAction> {
    let mut action = None;
    let all_selected = legacy_pane_has_target(pane, "0");

    ui.collapsing("编辑发送对象", |ui| {
        ui.horizontal_wrapped(|ui| {
            if let Some(next_action) = legacy_send_pane_target_checkbox(
                ui,
                pane,
                "0",
                "全员".to_owned(),
                true,
                "旧月莓频道0：发送给当前TRPG组的所有玩家私聊",
            ) {
                action = Some(next_action);
            }
            if ui.button("清空目标").clicked() {
                action = Some(
                    LegacyGroupSurfaceAction::ClearSendPaneTargets {
                        pane_key: pane.key.clone(),
                    },
                );
            }
        });
        if all_selected {
            ui.small("已选择全员，旧规则会折叠并禁用其他发送对象。");
        }

        let mut player_ids = group
            .players
            .iter()
            .filter(|target_id| !target_id.trim().is_empty())
            .collect::<Vec<_>>();
        player_ids.sort_by(|left, right| {
            target_display_name(manager, left)
                .cmp(&target_display_name(manager, right))
                .then_with(|| left.cmp(right))
        });
        if !player_ids.is_empty() {
            ui.label("目标");
            ui.horizontal_wrapped(|ui| {
                for target_id in player_ids {
                    let covered =
                        group.legacy_send_pane_direct_target_is_covered(&pane.key, target_id);
                    let enabled = !all_selected && !covered;
                    let mut label = target_display_name(manager, target_id);
                    if covered {
                        label.push_str("（已覆盖）");
                    }
                    let hover_text = if all_selected {
                        "已选择全员，旧规则禁用单独玩家目标"
                    } else if covered {
                        "这个PC已被选中的旧频道或虚拟讨论组覆盖"
                    } else {
                        "切换这个玩家私聊目标"
                    };
                    if let Some(next_action) = legacy_send_pane_target_checkbox(
                        ui, pane, target_id, label, enabled, hover_text,
                    ) {
                        action = Some(next_action);
                    }
                }
            });
        }

        let mut teams = group
            .legacy_teams
            .iter()
            .filter(|team| !team.id.trim().is_empty())
            .collect::<Vec<_>>();
        teams.sort_by(|left, right| {
            left.name
                .cmp(&right.name)
                .then_with(|| left.id.cmp(&right.id))
        });
        if !teams.is_empty() {
            ui.label("频道");
            ui.horizontal_wrapped(|ui| {
                for team in teams {
                    let label = if team.name.trim().is_empty() {
                        format!("频道 {}", team.id)
                    } else {
                        format!("{} ({})", team.name, team.id)
                    };
                    if let Some(next_action) = legacy_send_pane_target_checkbox(
                        ui,
                        pane,
                        &team.id,
                        label,
                        !all_selected,
                        if all_selected {
                            "已选择全员，旧规则禁用其他频道"
                        } else {
                            "切换这个旧频道目标"
                        },
                    ) {
                        action = Some(next_action);
                    }
                }
            });
        }

        let mut areas = Vec::new();
        for world in &group.legacy_worlds {
            for area in world.chat_areas.iter().chain(world.areas.iter()) {
                if !area.id.trim().is_empty() {
                    areas.push((world.name.as_str(), area));
                }
            }
        }
        areas.sort_by(|left, right| {
            left.0
                .cmp(right.0)
                .then_with(|| left.1.name.cmp(&right.1.name))
                .then_with(|| left.1.id.cmp(&right.1.id))
        });
        if !areas.is_empty() {
            ui.label("虚拟讨论组");
            ui.horizontal_wrapped(|ui| {
                for (world_name, area) in areas {
                    let area_kind = if area.combat { "战斗区" } else { "讨论组" };
                    let label = if area.name.trim().is_empty() {
                        format!("{area_kind} {} ({world_name})", area.id)
                    } else {
                        format!(
                            "{}：{} ({world_name})",
                            area_kind, area.name
                        )
                    };
                    if let Some(next_action) = legacy_send_pane_target_checkbox(
                        ui,
                        pane,
                        &area.id,
                        label,
                        !all_selected,
                        if all_selected {
                            "已选择全员，旧规则禁用虚拟讨论组"
                        } else {
                            "切换这个虚拟讨论组目标"
                        },
                    ) {
                        action = Some(next_action);
                    }
                }
            });
        }
    });

    action
}

fn legacy_group_surfaces_ui(
    ui: &mut Ui,
    manager: &NapcatMessageManager,
    state: &mut TrpgGroupSettingsState,
    chat_input_msgs: &mut Local<HashMap<String, String>>,
    napcat_sender: Option<&NapcatIOSender>,
    ime: &mut ImeManager,
    mut scene_store: Option<&mut Persistent<VoxelSceneStore>>,
    group_name: &str,
    group: &TrpgGroup,
) -> Option<LegacyGroupSurfaceAction> {
    let chat_area_count = group
        .legacy_worlds
        .iter()
        .map(|world| world.chat_areas.len() + world.areas.len())
        .sum::<usize>();
    if group.legacy_teams.is_empty()
        && group.legacy_worlds.is_empty()
        && group.legacy_send_panes.is_empty()
    {
        return None;
    }

    let mut action = None;
    ui.collapsing("月莓旧频道/世界", |ui| {
        ui.horizontal_wrapped(|ui| {
            ui.small(format!(
                "旧频道 {}",
                group.legacy_teams.len()
            ));
            ui.small(format!(
                "旧世界 {}",
                group.legacy_worlds.len()
            ));
            ui.small(format!(
                "虚拟讨论组 {}",
                chat_area_count
            ));
            ui.small(format!(
                "旧发送窗 {}",
                group.legacy_send_panes.len()
            ));
        });

        if !group.legacy_teams.is_empty() {
            ui.label("旧频道");
            for team in &group.legacy_teams {
                ui.horizontal_wrapped(|ui| {
                    ui.strong(&team.name);
                    ui.small(format!("ID {}", team.id));
                    if team.chat_message_count > 0 {
                        ui.small(format!(
                            "本地聊天 {}条",
                            team.chat_message_count
                        ));
                    }
                    if team.anonymous_speakers {
                        ui.small("匿名发言");
                    }
                    if team.allow_pc_nickname_repeat {
                        ui.small("允许重名");
                    }
                    if !team.visible {
                        ui.small("隐藏");
                    }
                    if team.window_x != 0.0
                        || team.window_y != 0.0
                        || team.window_width != 0.0
                        || team.window_height != 0.0
                    {
                        ui.small(format!(
                            "旧窗口 {:.0},{:.0} {:.0}x{:.0}",
                            team.window_x, team.window_y, team.window_width, team.window_height
                        ));
                    }
                    ui.small(legacy_member_preview(
                        manager,
                        &team.players,
                    ));
                    let window_key = (group_name.to_owned(), team.id.clone());
                    let window_open = state.open_legacy_team_chat_windows.contains(&window_key);
                    let button_text =
                        if window_open { "独立聊天窗已开" } else { "打开独立聊天窗" };
                    let response = ui.add_enabled(
                        !window_open,
                        egui::Button::new(button_text),
                    );
                    let clicked = response.clicked();
                    response.on_hover_text("打开为独立旧频道聊天窗");
                    if clicked {
                        state.open_legacy_team_chat_windows.insert(window_key);
                    }
                    if ui.button("转为小队").clicked() {
                        action = Some(LegacyGroupSurfaceAction::Promote(
                            LegacyPartyPromotion::Team(team.id.clone()),
                        ));
                    }
                });
                if !team.chat_messages.is_empty() || team.chat_message_count > 0 {
                    ui.indent(
                        format!("legacy_team_chat_{}", team.id),
                        |ui| {
                            ui.collapsing("旧频道本地聊天", |ui| {
                                if let Some(next_action) = legacy_team_chat_composer_ui(
                                    ui,
                                    manager,
                                    group,
                                    group_name,
                                    team,
                                    state,
                                    chat_input_msgs,
                                    napcat_sender,
                                    ime,
                                    2,
                                    220.0,
                                ) {
                                    action = Some(next_action);
                                }
                            });
                        },
                    );
                }
            }
        }

        for world in &group.legacy_worlds {
            let world_unit_ids = manager.unit_pool_ids_for_legacy_members(&world.npcs);
            let world_unit_status_id = format!(
                "legacy-world-units:{group_name}:{}",
                world.id
            );
            ui.separator();
            ui.horizontal_wrapped(|ui| {
                ui.label(format!("旧世界：{}", world.name));
                if !world.visible {
                    ui.small("隐藏");
                }
                if !world.players.is_empty() {
                    ui.small(format!(
                        "PC：{}",
                        legacy_member_preview(manager, &world.players)
                    ));
                }
                if !world.npcs.is_empty() {
                    ui.small(format!("NPC {}个", world.npcs.len()));
                    if world_unit_ids.is_empty() {
                        ui.small("未匹配单位模板");
                    } else {
                        ui.small(format!(
                            "匹配单位 {}个",
                            world_unit_ids.len()
                        ));
                    }
                }
                if let Some(store) = scene_store.as_deref_mut() {
                    if ui
                        .add_enabled(
                            !world_unit_ids.is_empty(),
                            egui::Button::new("放入世界NPC标记"),
                        )
                        .on_disabled_hover_text("旧世界NPC没有匹配的单位模板")
                        .clicked()
                    {
                        let status = place_legacy_world_unit_tokens_ui(
                            &mut *store,
                            manager,
                            group_name,
                            &world.id,
                            &world.name,
                            &world_unit_ids,
                            world.visible,
                        );
                        state
                            .legacy_area_marker_status
                            .insert(world_unit_status_id.clone(), status);
                    }
                    if ui.button("移出世界NPC标记").clicked() {
                        let status =
                            remove_legacy_world_unit_tokens_ui(&mut *store, group_name, &world.id);
                        state
                            .legacy_area_marker_status
                            .insert(world_unit_status_id.clone(), status);
                    }
                }
                if let Some(status) = state.legacy_area_marker_status.get(&world_unit_status_id) {
                    ui.small(status);
                }
            });
            for area in world.chat_areas.iter().chain(world.areas.iter()) {
                let marker_id = legacy_area_marker_id(group_name, &world.id, &area.id);
                let area_unit_ids = manager.unit_pool_ids_for_legacy_members(&area.members);
                let area_unit_status_id = format!("{marker_id}:units");
                ui.horizontal_wrapped(|ui| {
                    ui.small(format!(
                        "{}：{}",
                        if area.combat { "战斗区" } else { "讨论组" },
                        area.name
                    ));
                    ui.small(format!("ID {}", area.id));
                    ui.small(format!(
                        "成员：{}",
                        legacy_member_preview(manager, &area.members)
                    ));
                    if !area_unit_ids.is_empty() {
                        ui.small(format!(
                            "成员单位 {}个",
                            area_unit_ids.len()
                        ));
                    }
                    if ui.button("转为小队").clicked() {
                        action = Some(LegacyGroupSurfaceAction::Promote(
                            LegacyPartyPromotion::ChatArea(area.id.clone()),
                        ));
                    }
                    if let Some(store) = scene_store.as_deref_mut() {
                        let has_marker =
                            has_legacy_area_marker(store, group_name, &world.id, &area.id);
                        let label = if has_marker { "更新场景标记" } else { "放入场景" };
                        if ui.button(label).clicked() {
                            let status = match place_legacy_area_marker(
                                &mut *store,
                                group_name,
                                &world.id,
                                &world.name,
                                &area.id,
                                &area.name,
                                area.combat,
                                &area.members,
                                area.x,
                                area.y,
                                area.width,
                                area.height,
                                world.visible,
                            ) {
                                Ok(scene_changed) => match store.persist() {
                                    Ok(()) => {
                                        if scene_changed {
                                            "已写入场景标记".to_owned()
                                        } else {
                                            "场景标记已是最新".to_owned()
                                        }
                                    },
                                    Err(err) => format!("场景标记保存失败：{err}"),
                                },
                                Err(err) => format!("场景标记失败：{err}"),
                            };
                            state
                                .legacy_area_marker_status
                                .insert(marker_id.clone(), status);
                        }
                        if ui.button("写入体素边框").clicked() {
                            let status = match place_legacy_area_marker(
                                &mut *store,
                                group_name,
                                &world.id,
                                &world.name,
                                &area.id,
                                &area.name,
                                area.combat,
                                &area.members,
                                area.x,
                                area.y,
                                area.width,
                                area.height,
                                world.visible,
                            ) {
                                Ok(_) => match stamp_legacy_area_marker_voxel_outline(
                                    &mut *store,
                                    group_name,
                                    &world.id,
                                    &area.id,
                                ) {
                                    Ok(count) => match store.persist() {
                                        Ok(()) => format!("已写入体素边框 {count} 格"),
                                        Err(err) => format!("体素边框保存失败：{err}"),
                                    },
                                    Err(err) => format!("体素边框失败：{err}"),
                                },
                                Err(err) => format!("场景标记失败：{err}"),
                            };
                            state
                                .legacy_area_marker_status
                                .insert(marker_id.clone(), status);
                        }
                        if ui.button("写入体素填充").clicked() {
                            let status = match place_legacy_area_marker(
                                &mut *store,
                                group_name,
                                &world.id,
                                &world.name,
                                &area.id,
                                &area.name,
                                area.combat,
                                &area.members,
                                area.x,
                                area.y,
                                area.width,
                                area.height,
                                world.visible,
                            ) {
                                Ok(_) => match stamp_legacy_area_marker_voxel_fill(
                                    &mut *store,
                                    group_name,
                                    &world.id,
                                    &area.id,
                                ) {
                                    Ok(count) => match store.persist() {
                                        Ok(()) => format!("已写入体素填充 {count} 格"),
                                        Err(err) => format!("体素填充保存失败：{err}"),
                                    },
                                    Err(err) => format!("体素填充失败：{err}"),
                                },
                                Err(err) => format!("场景标记失败：{err}"),
                            };
                            state
                                .legacy_area_marker_status
                                .insert(marker_id.clone(), status);
                        }
                        if ui
                            .add_enabled(
                                !area_unit_ids.is_empty(),
                                egui::Button::new("放入成员单位标记"),
                            )
                            .on_disabled_hover_text("旧区域成员没有匹配的单位模板")
                            .clicked()
                        {
                            let status = place_legacy_area_unit_tokens_ui(
                                &mut *store,
                                manager,
                                group_name,
                                &world.id,
                                &area.id,
                                &area.name,
                                &area_unit_ids,
                                area.x,
                                area.y,
                                area.width,
                                area.height,
                                world.visible,
                            );
                            state
                                .legacy_area_marker_status
                                .insert(area_unit_status_id.clone(), status);
                        }
                        if ui.button("移出成员单位标记").clicked() {
                            let status = remove_legacy_area_unit_tokens_ui(
                                &mut *store,
                                group_name,
                                &world.id,
                                &area.id,
                            );
                            state
                                .legacy_area_marker_status
                                .insert(area_unit_status_id.clone(), status);
                        }
                        if has_marker && ui.button("移出场景").clicked() {
                            let removed = remove_legacy_area_marker(
                                &mut *store,
                                group_name,
                                &world.id,
                                &area.id,
                            );
                            let status = if removed {
                                match store.persist() {
                                    Ok(()) => "已移出场景标记".to_owned(),
                                    Err(err) => format!("移出场景标记保存失败：{err}"),
                                }
                            } else {
                                "场景里没有这个标记".to_owned()
                            };
                            state
                                .legacy_area_marker_status
                                .insert(marker_id.clone(), status);
                        }
                    } else {
                        ui.small("场景未就绪");
                    }
                    if let Some(status) = state.legacy_area_marker_status.get(&marker_id) {
                        ui.small(status);
                    }
                    if let Some(status) = state.legacy_area_marker_status.get(&area_unit_status_id)
                    {
                        ui.small(status);
                    }
                });
            }
        }

        ui.separator();
        ui.horizontal_wrapped(|ui| {
            ui.label("旧发送窗");
            if ui.button("新增多选发送窗").clicked() {
                action = Some(LegacyGroupSurfaceAction::AddSendPane);
            }
        });
        if group.legacy_send_panes.is_empty() {
            ui.small("还没有旧发送窗。");
        } else {
            for pane in &group.legacy_send_panes {
                ui.group(|ui| {
                    ui.set_width(ui.available_width());
                    legacy_send_pane_composer_ui(
                        ui,
                        manager,
                        group,
                        group_name,
                        pane,
                        napcat_sender,
                        chat_input_msgs,
                        &mut state.legacy_send_pane_status,
                        ime,
                        2,
                    );
                    if let Some(next_action) =
                        legacy_send_pane_target_editor_ui(ui, manager, group, pane)
                    {
                        action = Some(next_action);
                    }
                    ui.horizontal_wrapped(|ui| {
                        let window_key = (group_name.to_owned(), pane.key.clone());
                        let fixed_open = !pane.closable;
                        let window_open =
                            fixed_open || state.open_legacy_send_pane_windows.contains(&window_key);
                        let button_text =
                            if window_open { "独立窗已开" } else { "打开独立窗" };
                        let response = ui.add_enabled(
                            !fixed_open,
                            egui::Button::new(button_text),
                        );
                        let clicked = response.clicked();
                        response.on_hover_text(if fixed_open {
                            "这个旧发送窗会自动保持独立窗口"
                        } else {
                            "打开为独立浮动发送窗"
                        });
                        if clicked {
                            state.open_legacy_send_pane_windows.insert(window_key);
                        }
                        if pane.closable && ui.button("删除发送窗").clicked() {
                            action = Some(
                                LegacyGroupSurfaceAction::RemoveSendPane {
                                    pane_key: pane.key.clone(),
                                },
                            );
                        }
                    });
                });
            }
        }
    });
    action
}

#[derive(Clone, Copy)]
enum LegacyNegativeAction {
    Start,
    HalfWarning,
    Timeout,
    Reset,
}

fn legacy_negative_status_key(group_name: &str, target_id: &str) -> String {
    format!("{group_name}:{target_id}")
}

fn legacy_negative_send_input_id(group_name: &str, target_id: &str, action: &str) -> String {
    format!("legacy_negative:{group_name}:{target_id}:{action}")
}

fn legacy_negative_remaining_label(timer: Option<&TrpgLegacyNegativeTimer>) -> String {
    let remaining_ms = timer.map(|timer| timer.remaining_ms).unwrap_or_default();
    if remaining_ms == 0 {
        return "未计时".to_owned();
    }
    let seconds = remaining_ms.div_ceil(1000);
    format!("剩余{}秒", seconds)
}

fn legacy_negative_notice_text(action: LegacyNegativeAction) -> Option<&'static str> {
    match action {
        LegacyNegativeAction::Start => Some(
            "已经有一半的玩家进入下一回合, 消极倒计时2分钟, 2分钟内未有任何回复,将会直接跳过此回合,并叠加一层消极。",
        ),
        LegacyNegativeAction::HalfWarning => Some("消极倒计时已经过去一半,请尽快回复。"),
        LegacyNegativeAction::Timeout => Some("因为未回复, 你的回合已被跳过, 并叠加了一层消极"),
        LegacyNegativeAction::Reset => None,
    }
}

fn queue_legacy_negative_notice(
    group_name: &str,
    target_id: &str,
    action: LegacyNegativeAction,
    sender: Option<&NapcatIOSender>,
    ime: &mut ImeManager,
) -> Result<bool, String> {
    let Some(text) = legacy_negative_notice_text(action) else {
        return Ok(false);
    };
    let Some(sender) = sender else {
        return Ok(false);
    };
    let user_id = target_id
        .trim()
        .parse::<u64>()
        .map_err(|_| format!("消极目标不是有效QQ号：{target_id}"))?;
    let action_label = match action {
        LegacyNegativeAction::Start => "start",
        LegacyNegativeAction::HalfWarning => "half",
        LegacyNegativeAction::Timeout => "timeout",
        LegacyNegativeAction::Reset => "reset",
    };
    ime.queue_text_send(
        &legacy_negative_send_input_id(group_name, target_id, action_label),
        text,
        sender,
        vec![NapcatSendTarget::Private(user_id)],
    )?;
    Ok(true)
}

fn trpg_group_settings_window(
    ctx: &Context,
    manager: &mut ResMut<Persistent<NapcatMessageManager>>,
    deepseek_manager: &mut ResMut<Persistent<DeepseekManager>>,
    mut scene_store: Option<&mut Persistent<VoxelSceneStore>>,
    scene_runtime: Option<&mut VoxelMapRuntimeState>,
    battle_store: Option<&mut Persistent<BattleRoundStore>>,
    napcat_sender: Option<&NapcatIOSender>,
    ime: &mut ImeManager,
    chat_input_msgs: &mut Local<HashMap<String, String>>,
    state: &mut TrpgGroupSettingsState,
    character_edit_state: &mut CharacterEditState,
    rule_engine_state: &mut RuleEngineState,
) {
    if !state.open {
        return;
    }

    let player_targets = sorted_pool_targets(manager, false);
    let group_chat_targets = sorted_pool_targets(manager, true);
    let mut changed = false;
    let mut group_to_delete = None;
    let mut character_to_delete = None;
    let mut turn_action: Option<(String, String, bool)> = None;
    let mut turn_reset: Option<String> = None;
    let mut turn_advance: Option<String> = None;
    let mut turn_zero_reset: Option<String> = None;
    let mut initial_stats_restore: Option<String> = None;
    let mut legacy_negative_action: Option<(String, String, LegacyNegativeAction)> = None;
    let mut legacy_surface_action: Option<(String, LegacyGroupSurfaceAction)> = None;
    let mut settings_open = state.open;

    egui::Window::new("TRPG设置")
        .id(Id::new("trpg_group_settings_window"))
        .open(&mut settings_open)
        .default_size(Vec2::new(620.0, 520.0))
        .min_width(420.0)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("TRPG组");
                ui.text_edit_singleline(&mut state.new_group_name);
                if ui.button("创建").clicked() {
                    let name = state.new_group_name.trim();
                    if manager.create_trpg_group(name) {
                        if manager.current_trpg_group.is_none() {
                            manager.current_trpg_group = Some(name.to_owned());
                        }
                        state.new_group_name.clear();
                        changed = true;
                    }
                }
            });

            napcat_import_export_ui(
                ui,
                manager,
                deepseek_manager,
                scene_store.as_deref_mut(),
                scene_runtime,
                battle_store,
                state,
            );
            ui.separator();

            ui.heading("玩家角色");
            if player_targets.is_empty() {
                ui.label("还没有玩家私聊。");
            } else {
                egui::ScrollArea::vertical()
                    .id_salt("player_character_settings")
                    .max_height(260.0)
                    .show(ui, |ui| {
                        for target_id in &player_targets {
                            let display_name = target_display_name(manager, target_id);
                            let skill_pool_snapshot = manager.skill_pool.clone();
                            let item_pool_snapshot = manager.item_pool.clone();
                            let stat_config = manager.character_stat_config_for_target(target_id);
                            let character = manager
                                .player_characters
                                .entry(target_id.clone())
                                .or_default();
                            let pending_skill_count = pending_gm_skill_count(character);
                            let character_label = if pending_skill_count == 0 {
                                format!("{display_name} ({target_id})")
                            } else {
                                format!(
                                    "{display_name} ({target_id}) · 待GM确认技能 {pending_skill_count}"
                                )
                            };
                            ui.collapsing(character_label, |ui| {
                                    character_status_summary_ui(ui, character);
                                    ui.horizontal(|ui| {
                                        let pending_delete =
                                            state.pending_character_delete.as_deref()
                                                == Some(target_id.as_str());
                                        if pending_delete {
                                            ui.label("确认删除？");
                                            if ui.button("删除角色").clicked() {
                                                character_to_delete = Some(target_id.clone());
                                            }
                                            if ui.button("取消").clicked() {
                                                state.pending_character_delete = None;
                                            }
                                        } else if ui.button("删除角色").clicked() {
                                            state.pending_character_delete =
                                                Some(target_id.clone());
                                        }
                                    });
                                    ui.separator();
                                    ui.collapsing("编辑角色", |ui| {
                                        changed |= character_editor_ui(
                                            ui,
                                            target_id,
                                            character,
                                            &display_name,
                                            character_edit_state,
                                            rule_engine_state,
                                            &skill_pool_snapshot,
                                            &item_pool_snapshot,
                                            stat_config,
                                        );
                                    });
                                });
                        }
                    });
            }

            ui.separator();
            ui.heading("TRPG组成员");

            let mut group_names = manager.trpg_groups.keys().cloned().collect::<Vec<_>>();
            group_names.sort();
            if group_names.is_empty() {
                ui.label("先创建TRPG组，再分配玩家和群聊。");
                return;
            }

            let mut current_group = manager.current_trpg_group.clone().unwrap_or_default();
            egui::ComboBox::from_label("当前TRPG组")
                .selected_text(if current_group.is_empty() {
                    "无"
                } else {
                    current_group.as_str()
                })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut current_group, String::new(), "无");
                    for group_name in &group_names {
                        ui.selectable_value(
                            &mut current_group,
                            group_name.clone(),
                            group_name,
                        );
                    }
                });
            let next_current_group = (!current_group.is_empty()).then_some(current_group);
            if manager.current_trpg_group != next_current_group {
                manager.current_trpg_group = next_current_group;
                changed = true;
            }
            ui.add_space(6.0);

            ui.collapsing("随机池", |ui| {
                egui::ScrollArea::vertical()
                    .id_salt("trpg_settings_random_pool")
                    .max_height(420.0)
                    .show(ui, |ui| {
                        changed |= random_pool_settings_ui(
                            ui,
                            manager,
                            state,
                            &player_targets,
                            napcat_sender,
                            Some(&mut *ime),
                        );
                    });
            });
            ui.add_space(6.0);

            egui::ScrollArea::vertical()
                .id_salt("trpg_group_membership_settings")
                .show(ui, |ui| {
                    for group_name in group_names {
                        let Some(snapshot) = manager.trpg_groups.get(&group_name).cloned() else {
                            continue;
                        };
                        let group_response = ui.group(|ui| {
                            ui.set_width(ui.available_width());
                            ui.horizontal(|ui| {
                                ui.heading(&group_name);
                                ui.small(format!(
                                    "{}个目标，世界轮次{}",
                                    trpg_group_member_count(&snapshot),
                                    snapshot.world_turn
                                ));
                                ui.with_layout(
                                    egui::Layout::right_to_left(egui::Align::Center),
                                    |ui| {
                                        if ui.button("删除").clicked() {
                                            group_to_delete = Some(group_name.clone());
                                        }
                                    },
                                );
                            });

                            ui.horizontal_wrapped(|ui| {
                                if ui.button("推进轮次").clicked() {
                                    turn_advance = Some(group_name.clone());
                                }
                                if ui.button("重置行动").clicked() {
                                    turn_reset = Some(group_name.clone());
                                }
                                if state.pending_turn_zero_reset.as_deref()
                                    == Some(group_name.as_str())
                                {
                                    if ui
                                        .button("确认轮次全部归零")
                                        .on_hover_text("世界轮次和所有玩家轮次都会设为0")
                                        .clicked()
                                    {
                                        turn_zero_reset = Some(group_name.clone());
                                        state.pending_turn_zero_reset = None;
                                    }
                                    if ui.button("取消").clicked() {
                                        state.pending_turn_zero_reset = None;
                                    }
                                } else if ui
                                    .button("轮次全部归零")
                                    .on_hover_text("需要再次确认；不会自动恢复角色状态")
                                    .clicked()
                                {
                                    state.pending_turn_zero_reset = Some(group_name.clone());
                                }
                                if state.pending_initial_stats_restore.as_deref()
                                    == Some(group_name.as_str())
                                {
                                    if ui
                                        .button("确认恢复首轮前状态")
                                        .on_hover_text(
                                            "恢复所有玩家在第一次行动前保存的HP、MP、属性、buff和冷却状态",
                                        )
                                        .clicked()
                                    {
                                        initial_stats_restore = Some(group_name.clone());
                                        state.pending_initial_stats_restore = None;
                                    }
                                    if ui.button("取消").clicked() {
                                        state.pending_initial_stats_restore = None;
                                    }
                                } else if ui
                                    .button("恢复首轮前状态")
                                    .on_hover_text("需要再次确认；不会使用建卡完成时的默认值")
                                    .clicked()
                                {
                                    state.pending_initial_stats_restore =
                                        Some(group_name.clone());
                                }
                            });
                            if let Some(status) = state.group_reset_status.get(&group_name) {
                                ui.small(status);
                            }

                            ui.collapsing("团设与建卡规则", |ui| {
                                if let Some(group) = manager.trpg_groups.get_mut(&group_name) {
                                    ui.horizontal(|ui| {
                                        ui.label("活动ID");
                                        ui.monospace(&group.campaign_id).on_hover_text(
                                            "活动ID用于隔离私聊、队伍消息、摘要和战斗记录，创建后不可直接修改。",
                                        );
                                        ui.label("初始属性点");
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(
                                                    &mut group.initial_status_points,
                                                )
                                                .range(0..=999),
                                            )
                                            .changed();
                                        ui.label("初始技能点");
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(
                                                    &mut group.initial_exchange_points,
                                                )
                                                .range(0..=999),
                                            )
                                            .changed();
                                        changed |= ui
                                            .checkbox(
                                                &mut group.allow_join_requests,
                                                "允许入团请求",
                                            )
                                            .changed();
                                        changed |= ui
                                            .add(
                                                egui::DragValue::new(&mut group.run_times)
                                                    .range(0..=9999)
                                                    .prefix("开团次数 "),
                                            )
                                            .changed();
                                    });
                                    ui.horizontal_wrapped(|ui| {
                                        changed |= ui
                                            .checkbox(
                                                &mut group.battle_sort_by_turn,
                                                "新战斗按行动排序",
                                            )
                                            .changed();
                                        if ui
                                            .checkbox(
                                                &mut group.battle_negative_enabled,
                                                "新战斗启用消极",
                                            )
                                            .changed()
                                        {
                                            changed = true;
                                            changed |= group.sync_legacy_negative_timers();
                                        }
                                        if group.legacy_negative_count > 0 {
                                            ui.small(format!(
                                                "旧消极记录 {} 条",
                                                group.legacy_negative_count
                                            ));
                                        }
                                        if !group.legacy_negative_timers.is_empty() {
                                            ui.small(format!(
                                                "计时器 {} 个",
                                                group.legacy_negative_timers.len()
                                            ));
                                        }
                                    });

                                    ui.label("公开说明");
                                    changed |= ui
                                        .add(
                                            egui::TextEdit::multiline(&mut group.description)
                                                .desired_width(ui.available_width())
                                                .desired_rows(2),
                                        )
                                        .changed();
                                    ui.label("GM说明");
                                    changed |= ui
                                        .add(
                                            egui::TextEdit::multiline(&mut group.st_description)
                                                .desired_width(ui.available_width())
                                                .desired_rows(2),
                                        )
                                        .changed();
                                    ui.label("玩家引导");
                                    changed |= ui
                                        .add(
                                            egui::TextEdit::multiline(&mut group.guide)
                                                .desired_width(ui.available_width())
                                                .desired_rows(3),
                                        )
                                        .changed();
                                    ui.collapsing("属性公式", |ui| {
                                        changed |=
                                            trpg_basic_config_ui(ui, &mut group.basic_config);
                                    });
                                }
                            });

                            if snapshot.players.is_empty() {
                                ui.small("这个TRPG轮次组里没有玩家。");
                            } else {
                                ui.label("轮次状态");
                                for target_id in &snapshot.players {
                                    let turn = snapshot.player_turns.get(target_id);
                                    let turns_passed =
                                        turn.map(|turn| turn.turns_passed).unwrap_or_default();
                                    let acted = turn.map(|turn| turn.acted).unwrap_or_default();
                                    let skipped = turn.map(|turn| turn.skipped).unwrap_or_default();
                                    let negative_timer = snapshot.legacy_negative_timer(target_id);
                                    let negative_active =
                                        negative_timer.is_some_and(|timer| timer.active());
                                    let negative_half_warned =
                                        negative_timer.is_some_and(|timer| timer.half_warned);
                                    let negative_layers = negative_timer
                                        .map(|timer| timer.negative_layers)
                                        .unwrap_or_default();
                                    let status = if acted {
                                        "已行动"
                                    } else if skipped {
                                        "已跳过"
                                    } else {
                                        "等待中"
                                    };
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label(target_display_name(manager, target_id));
                                        ui.small(format!("{}轮", turns_passed));
                                        ui.small(status);
                                        if ui.button("行动").clicked() {
                                            turn_action = Some((
                                                group_name.clone(),
                                                target_id.clone(),
                                                true,
                                            ));
                                        }
                                        if ui.button("跳过").clicked() {
                                            turn_action = Some((
                                                group_name.clone(),
                                                target_id.clone(),
                                                false,
                                            ));
                                        }
                                        if snapshot.battle_negative_enabled
                                            || negative_timer.is_some()
                                        {
                                            ui.small(legacy_negative_remaining_label(
                                                negative_timer,
                                            ));
                                            if negative_layers > 0 {
                                                ui.small(format!("消极{}层", negative_layers));
                                            }
                                            if negative_active {
                                                ui.small(if negative_half_warned {
                                                    "已半程提醒"
                                                } else {
                                                    "倒计时中"
                                                });
                                            } else if negative_timer
                                                .is_some_and(|timer| timer.replied)
                                            {
                                                ui.small("已回复");
                                            }
                                            if ui.button("启动倒计时").clicked() {
                                                legacy_negative_action = Some((
                                                    group_name.clone(),
                                                    target_id.clone(),
                                                    LegacyNegativeAction::Start,
                                                ));
                                            }
                                            if ui
                                                .add_enabled(
                                                    negative_active && !negative_half_warned,
                                                    egui::Button::new("半程提醒"),
                                                )
                                                .clicked()
                                            {
                                                legacy_negative_action = Some((
                                                    group_name.clone(),
                                                    target_id.clone(),
                                                    LegacyNegativeAction::HalfWarning,
                                                ));
                                            }
                                            if ui.button("触发消极").clicked() {
                                                legacy_negative_action = Some((
                                                    group_name.clone(),
                                                    target_id.clone(),
                                                    LegacyNegativeAction::Timeout,
                                                ));
                                            }
                                            if ui.button("重置计时").clicked() {
                                                legacy_negative_action = Some((
                                                    group_name.clone(),
                                                    target_id.clone(),
                                                    LegacyNegativeAction::Reset,
                                                ));
                                            }
                                            if let Some(status) =
                                                state.legacy_negative_status.get(
                                                    &legacy_negative_status_key(
                                                        &group_name,
                                                        target_id,
                                                    ),
                                                )
                                            {
                                                ui.small(status);
                                            }
                                        }
                                    });
                                }
                                ui.separator();
                            }

                            ui.collapsing("小队与可见性", |ui| {
                                let draft = state
                                    .party_name_drafts
                                    .entry(group_name.clone())
                                    .or_default();
                                ui.horizontal(|ui| {
                                    ui.label("新小队");
                                    ui.text_edit_singleline(draft);
                                    if ui.button("创建小队").clicked() {
                                        let party_name = draft.trim().to_owned();
                                        if !party_name.is_empty() {
                                            if let Some(group) =
                                                manager.trpg_groups.get_mut(&group_name)
                                            {
                                                changed |= group.ensure_party(&party_name);
                                            }
                                            draft.clear();
                                        }
                                    }
                                });

                                let mut party_names =
                                    snapshot.parties.keys().cloned().collect::<Vec<_>>();
                                party_names.sort();

                                if party_names.is_empty() {
                                    ui.small("还没有小队。");
                                } else {
                                    ui.label("小队管理");
                                    for party_name in &party_names {
                                        let members = snapshot
                                            .parties
                                            .get(party_name)
                                            .map(|party| party.players.clone())
                                            .unwrap_or_default();
                                        let member_label =
                                            if members.is_empty() {
                                                "无成员".to_owned()
                                            } else {
                                                members
                                                    .iter()
                                                    .map(|target_id| {
                                                        target_display_name(manager, target_id)
                                                    })
                                                    .collect::<Vec<_>>()
                                                    .join("、")
                                            };
                                        let merge_options = party_names
                                            .iter()
                                            .filter(|candidate| *candidate != party_name)
                                            .cloned()
                                            .collect::<Vec<_>>();
                                        let merge_key = (group_name.clone(), party_name.clone());
                                        let mut merge_target = state
                                            .party_merge_targets
                                            .get(&merge_key)
                                            .cloned()
                                            .unwrap_or_default();
                                        if !merge_options
                                            .iter()
                                            .any(|candidate| candidate == &merge_target)
                                        {
                                            merge_target =
                                                merge_options.first().cloned().unwrap_or_default();
                                        }
                                        let pending_delete =
                                            state.pending_party_delete.as_ref()
                                                == Some(&merge_key);
                                        let mut clear_merge_key = false;

                                        ui.horizontal_wrapped(|ui| {
                                            ui.strong(party_name);
                                            ui.small(format!("成员：{member_label}"));
                                            if merge_options.is_empty() {
                                                ui.add_enabled(
                                                    false,
                                                    egui::Button::new("合并"),
                                                )
                                                .on_hover_text("至少需要两个小队");
                                            } else {
                                                egui::ComboBox::from_id_salt((
                                                    "party_merge_target",
                                                    &group_name,
                                                    party_name,
                                                ))
                                                .selected_text(merge_target.as_str())
                                                .show_ui(
                                                    ui,
                                                    |ui| {
                                                        for candidate in &merge_options {
                                                            ui.selectable_value(
                                                                &mut merge_target,
                                                                candidate.clone(),
                                                                candidate,
                                                            );
                                                        }
                                                    },
                                                );
                                                if ui
                                                    .button("合并到")
                                                    .on_hover_text(
                                                        "把这个小队的成员移动到目标小队",
                                                    )
                                                    .clicked()
                                                {
                                                    if let Some(group) =
                                                        manager.trpg_groups.get_mut(&group_name)
                                                    {
                                                        changed |= group.merge_party(
                                                            party_name,
                                                            &merge_target,
                                                        );
                                                    }
                                                    state
                                                        .party_merge_targets
                                                        .remove(&merge_key);
                                                    clear_merge_key = true;
                                                    if state.pending_party_delete.as_ref()
                                                        == Some(&merge_key)
                                                    {
                                                        state.pending_party_delete = None;
                                                    }
                                                }
                                            }

                                            if pending_delete {
                                                ui.label("确认删除？");
                                                if ui
                                                    .button("删除小队")
                                                    .on_hover_text("成员会变为无小队")
                                                    .clicked()
                                                {
                                                    if let Some(group) =
                                                        manager.trpg_groups.get_mut(&group_name)
                                                    {
                                                        changed |= group.remove_party(party_name);
                                                    }
                                                    state.pending_party_delete = None;
                                                    state
                                                        .party_merge_targets
                                                        .remove(&merge_key);
                                                    clear_merge_key = true;
                                                }
                                                if ui.button("取消").clicked() {
                                                    state.pending_party_delete = None;
                                                }
                                            } else if ui.button("删除").clicked() {
                                                state.pending_party_delete =
                                                    Some(merge_key.clone());
                                            }
                                        });

                                        if !clear_merge_key && !merge_target.is_empty() {
                                            state
                                                .party_merge_targets
                                                .insert(merge_key, merge_target);
                                        }
                                    }
                                    ui.separator();
                                }

                                if snapshot.players.is_empty() {
                                    ui.small("这个TRPG组里没有可分配的小队玩家。");
                                } else {
                                    for target_id in &snapshot.players {
                                        let display_name = target_display_name(manager, target_id);
                                        let mut selected_party = snapshot
                                            .party_id_for_player(target_id)
                                            .unwrap_or_default()
                                            .to_owned();
                                        let before_party = selected_party.clone();
                                        ui.horizontal_wrapped(|ui| {
                                            ui.label(display_name);
                                            egui::ComboBox::from_id_salt((
                                                "party_assignment",
                                                &group_name,
                                                target_id,
                                            ))
                                            .selected_text(if selected_party.is_empty() {
                                                "无小队"
                                            } else {
                                                selected_party.as_str()
                                            })
                                            .show_ui(
                                                ui,
                                                |ui| {
                                                    ui.selectable_value(
                                                        &mut selected_party,
                                                        String::new(),
                                                        "无小队",
                                                    );
                                                    for party_name in &party_names {
                                                        ui.selectable_value(
                                                            &mut selected_party,
                                                            party_name.clone(),
                                                            party_name,
                                                        );
                                                    }
                                                },
                                            );

                                            if let Ok(user_id) = target_id.parse::<u64>() {
                                                let mut is_gm =
                                                    snapshot.gm_users.contains(&user_id);
                                                if ui.checkbox(&mut is_gm, "GM").changed() {
                                                    if let Some(group) =
                                                        manager.trpg_groups.get_mut(&group_name)
                                                    {
                                                        if is_gm {
                                                            changed |=
                                                                group.gm_users.insert(user_id);
                                                        } else {
                                                            changed |=
                                                                group.gm_users.remove(&user_id);
                                                        }
                                                    }
                                                }
                                            }
                                        });

                                        if selected_party != before_party {
                                            if let Some(group) =
                                                manager.trpg_groups.get_mut(&group_name)
                                            {
                                                changed |= group.set_player_party(
                                                    target_id,
                                                    (!selected_party.is_empty())
                                                        .then_some(selected_party.as_str()),
                                                );
                                            }
                                        }
                                    }
                                }
                            });

                            if let Some(promotion) = legacy_group_surfaces_ui(
                                ui,
                                manager,
                                state,
                                chat_input_msgs,
                                napcat_sender,
                                ime,
                                scene_store.as_deref_mut(),
                                &group_name,
                                &snapshot,
                            ) {
                                legacy_surface_action = Some((group_name.clone(), promotion));
                            }

                            ui.columns(2, |columns| {
                                columns[0].label("玩家");
                                for target_id in &player_targets {
                                    let mut selected = snapshot.players.contains(target_id);
                                    if columns[0]
                                        .checkbox(
                                            &mut selected,
                                            target_display_name(manager, target_id),
                                        )
                                        .on_hover_text(target_id)
                                        .changed()
                                    {
                                        if let Some(group) =
                                            manager.trpg_groups.get_mut(&group_name)
                                        {
                                            set_target_membership(
                                                &mut group.players,
                                                target_id,
                                                selected,
                                            );
                                            group.sync_turn_players();
                                            group.sync_legacy_negative_timers();
                                            group.sync_parties();
                                            changed = true;
                                        }
                                    }
                                }

                                columns[1].label("群聊");
                                for target_id in &group_chat_targets {
                                    let mut selected = snapshot.group_chats.contains(target_id);
                                    if columns[1]
                                        .checkbox(
                                            &mut selected,
                                            target_display_name(manager, target_id),
                                        )
                                        .on_hover_text(target_id)
                                        .changed()
                                    {
                                        if let Some(group) =
                                            manager.trpg_groups.get_mut(&group_name)
                                        {
                                            set_target_membership(
                                                &mut group.group_chats,
                                                target_id,
                                                selected,
                                            );
                                            changed = true;
                                        }
                                    }
                                }
                            });
                        });
                        if state.focused_group_name.as_deref() == Some(group_name.as_str()) {
                            group_response
                                .response
                                .scroll_to_me(Some(egui::Align::Center));
                            state.focused_group_name = None;
                        }
                        ui.add_space(6.0);
                    }
                });
        });
    state.open = settings_open;

    if let Some((group_name, target_id, acted)) = turn_action {
        changed |= mark_group_player_turn(
            manager.as_mut(),
            &group_name,
            &target_id,
            acted,
            rule_engine_state,
        );
    }
    if let Some(group_name) = turn_reset {
        if let Some(group) = manager.trpg_groups.get_mut(&group_name) {
            changed |= group.reset_current_turn();
        }
    }
    if let Some(group_name) = turn_advance {
        changed |= advance_group_world_turn(
            manager.as_mut(),
            &group_name,
            rule_engine_state,
        );
    }
    if let Some(group_name) = turn_zero_reset {
        let captured = capture_missing_group_initial_player_states(
            manager.as_mut(),
            &group_name,
        );
        let reset = manager
            .trpg_groups
            .get_mut(&group_name)
            .is_some_and(TrpgGroup::reset_all_turns);
        changed |= captured > 0 || reset;
        state.group_reset_status.insert(
            group_name,
            if captured == 0 {
                "轮次已全部归零；已有首轮前状态快照保持不变".to_owned()
            } else {
                format!("轮次已全部归零；为 {captured} 个玩家建立了新的首轮前状态快照")
            },
        );
    }
    if let Some(group_name) = initial_stats_restore {
        let (restored, missing) = restore_group_initial_player_stats(
            manager.as_mut(),
            &group_name,
            rule_engine_state,
        );
        changed |= restored > 0;
        state.group_reset_status.insert(
            group_name,
            if restored == 0 {
                "没有可恢复的首轮前状态；旧跑团请先确认一次“轮次全部归零”以建立基线"
                    .to_owned()
            } else if missing == 0 {
                format!("已恢复 {restored} 个玩家的首轮前状态")
            } else {
                format!("已恢复 {restored} 个玩家；另有 {missing} 个玩家没有首轮前快照")
            },
        );
    }
    if let Some((group_name, target_id, action)) = legacy_negative_action {
        let action_changed =
            match action {
                LegacyNegativeAction::Start => manager
                    .trpg_groups
                    .get_mut(&group_name)
                    .is_some_and(|group| {
                        group.start_legacy_negative_timer(&target_id, LEGACY_NEGATIVE_TIMEOUT_MS)
                    }),
                LegacyNegativeAction::HalfWarning => manager
                    .trpg_groups
                    .get_mut(&group_name)
                    .is_some_and(|group| group.mark_legacy_negative_half_warned(&target_id)),
                LegacyNegativeAction::Timeout => {
                    let timer_changed = manager
                        .trpg_groups
                        .get_mut(&group_name)
                        .is_some_and(|group| group.record_legacy_negative_timeout(&target_id));
                    let turn_changed = if timer_changed {
                        mark_group_player_turn(
                            manager.as_mut(),
                            &group_name,
                            &target_id,
                            false,
                            rule_engine_state,
                        )
                    } else {
                        false
                    };
                    timer_changed || turn_changed
                },
                LegacyNegativeAction::Reset => manager
                    .trpg_groups
                    .get_mut(&group_name)
                    .is_some_and(|group| group.reset_legacy_negative_timer(&target_id)),
            };
        changed |= action_changed;

        let status_key = legacy_negative_status_key(&group_name, &target_id);
        let status = if !action_changed {
            "没有可更新的消极计时".to_owned()
        } else if legacy_negative_notice_text(action).is_some() {
            match queue_legacy_negative_notice(
                &group_name,
                &target_id,
                action,
                napcat_sender,
                ime,
            ) {
                Ok(true) => "已更新，提醒已加入私聊发送队列".to_owned(),
                Ok(false) => "已更新本地计时；NapCat未连接，提醒未发送".to_owned(),
                Err(err) => format!("已更新本地计时；提醒发送失败：{err}"),
            }
        } else {
            "已更新本地计时".to_owned()
        };
        state.legacy_negative_status.insert(status_key, status);
    }
    if let Some((group_name, action)) = legacy_surface_action {
        changed |= apply_legacy_group_surface_action(
            manager.as_mut(),
            state,
            &group_name,
            action,
        );
    }

    if let Some(group_name) = group_to_delete {
        manager.trpg_groups.remove(&group_name);
        if manager.current_trpg_group.as_deref() == Some(group_name.as_str()) {
            manager.current_trpg_group = None;
        }
        changed = true;
    }
    if let Some(target_id) = character_to_delete {
        manager
            .player_characters
            .insert(target_id, PlayerCharacter::default());
        state.pending_character_delete = None;
        changed = true;
    }

    if changed {
        manager.persist().ok();
    }
}

pub fn ui_system(
    mut contexts: EguiContexts,
    mut ime: ResMut<ImeManager>,
    napcat_sender: Option<Res<NapcatIOSender>>,
    deepseek_sender: Option<Res<DeepseekIOSender>>,
    mut deepseek_manager: ResMut<Persistent<DeepseekManager>>,
    mut send_manager: ResMut<NapcatSendManager>,
    mut manager: ResMut<Persistent<NapcatMessageManager>>,
    mut cached_memory: ResMut<Persistent<CachedMemory>>,
    mut locals: UiSystemLocals,
    mut rule_engine_state: ResMut<RuleEngineState>,
    mut battle_round_state: ResMut<BattleRoundUiState>,
    scene_positions: Option<Res<SceneCharacterPositions>>,
    player_camera_positions: Option<Res<ScenePlayerCameraPositions>>,
    mut scene_store: Option<ResMut<Persistent<VoxelSceneStore>>>,
    mut scene_runtime: Option<ResMut<VoxelMapRuntimeState>>,
    mut player_view_request: Option<ResMut<ScenePlayerViewRequest>>,
) {
    let has_run_once: &mut Local<bool> = &mut locals.has_run_once;
    let new_chat_group_modal_string_open: &mut Local<(String, bool)> =
        &mut locals.new_chat_group_modal_string_open;
    let chat_input_msgs: &mut Local<HashMap<String, String>> = &mut locals.chat_input_msgs;
    let chat_scroll_states: &mut Local<HashMap<String, ChatScrollState>> =
        &mut locals.chat_scroll_states;
    let previous_group_rects: &mut Local<HashMap<String, Rect>> = &mut locals.previous_group_rects;
    let chat_list_edit_target: &mut Local<Option<String>> = &mut locals.chat_list_edit_target;
    let chat_list_edit_name: &mut Local<String> = &mut locals.chat_list_edit_name;
    let trpg_group_settings: &mut Local<TrpgGroupSettingsState> = &mut locals.trpg_group_settings;
    let character_edit_state: &mut Local<CharacterEditState> = &mut locals.character_edit_state;
    let quick_character_targets: &mut Local<HashSet<String>> = &mut locals.quick_character_targets;
    let image_textures: &mut Local<HashMap<String, TextureHandle>> =
        &mut locals.chat_image_textures;
    let turn_count_drafts: &mut Local<HashMap<(String, String), u32>> =
        &mut locals.chat_turn_count_drafts;
    let group_broadcast_scopes: &mut Local<HashMap<String, String>> =
        &mut locals.group_broadcast_scopes;
    let chat_player_visible_previews: &mut Local<HashMap<String, String>> =
        &mut locals.chat_player_visible_previews;
    let chat_list_player_visible_filter: &mut Local<Option<String>> =
        &mut locals.chat_list_player_visible_filter;
    let voxel_editor: &mut VoxelEditorState = &mut locals.voxel_editor;
    let voxel_possession: &mut VoxelPossessionState = &mut locals.voxel_possession;
    let battle_store = &mut locals.battle_store;
    let player_standees = &locals.player_standees;

    let Ok(ctx) = contexts.ctx_mut() else {
        return;
    };
    if manager.migrate_chat_window_state()
        || manager.sync_chat_targets()
        || manager.sync_skill_pool_from_completed_characters()
    {
        manager.persist().ok();
    }
    let napcat_sender = napcat_sender.as_deref();
    let deepseek_sender = deepseek_sender.as_deref();
    let mut sent_message_added = false;
    for completion in ime.apply_send_results(send_manager.results.drain(..)) {
        if completion.clear_input {
            if let Some(text) = chat_input_msgs.get_mut(&completion.input_id) {
                if text.trim() == completion.text {
                    text.clear();
                }
            }
        }
        for target in completion.successful_targets {
            if append_local_sent_message(&mut manager, target, &completion.text) {
                sent_message_added = true;
            }
        }
    }
    if sent_message_added {
        if let Err(err) = manager.persist() {
            eprintln!("failed to persist local sent NapCat message: {err}");
        }
    }

    let mut group_rects: HashMap<String, Rect> = HashMap::default();
    let mut group_deltas: HashMap<String, Vec2> = HashMap::default();
    let mut latest_group_rects: HashMap<String, Rect> = HashMap::default();
    let reset_data = |new_chat_group_modal_string_bool: &mut Local<'_, (String, bool)>| {
        new_chat_group_modal_string_bool.0 = "".to_owned();
        new_chat_group_modal_string_bool.1 = false;
    };

    if new_chat_group_modal_string_open.1 {
        let modal = Modal::new(Id::new("New Chat Group")).show(ctx, |ui| {
            ui.set_width(250.0);

            ui.heading("新建讨论组");
            ui.label("名称：");
            ui.text_edit_singleline(&mut new_chat_group_modal_string_open.0);

            egui::Sides::new().show(
                ui,
                |_ui| {},
                |ui| {
                    if ui.button("保存").clicked() {
                        manager.groups.insert(
                            new_chat_group_modal_string_open.0.to_owned(),
                            ChatGroup { members: vec![] },
                        );
                        manager.persist().ok();
                        reset_data(new_chat_group_modal_string_open);
                    }
                    if ui.button("取消").clicked() {
                        reset_data(new_chat_group_modal_string_open);
                    }
                },
            );
        });

        if modal.should_close() {
            reset_data(new_chat_group_modal_string_open);
        }
    }

    trpg_group_settings_window(
        ctx,
        &mut manager,
        &mut deepseek_manager,
        scene_store.as_deref_mut(),
        scene_runtime.as_deref_mut(),
        battle_store.as_deref_mut(),
        napcat_sender,
        &mut *ime,
        chat_input_msgs,
        trpg_group_settings,
        character_edit_state,
        &mut rule_engine_state,
    );
    legacy_send_pane_windows(
        ctx,
        &manager,
        napcat_sender,
        chat_input_msgs,
        trpg_group_settings,
        &mut ime,
    );
    if let Some((group_name, action)) = legacy_team_chat_windows(
        ctx,
        &manager,
        napcat_sender,
        chat_input_msgs,
        trpg_group_settings,
        &mut ime,
    ) {
        if apply_legacy_group_surface_action(
            manager.as_mut(),
            trpg_group_settings,
            &group_name,
            action,
        ) {
            manager.persist().ok();
        }
    }
    quick_character_windows(
        ctx,
        &mut manager,
        quick_character_targets,
        character_edit_state,
        &mut rule_engine_state,
        scene_positions.as_deref(),
        player_camera_positions.as_deref(),
    );
    pool_management_window(
        ctx,
        &mut manager,
        trpg_group_settings,
        napcat_sender,
        &mut ime,
        scene_store.as_deref_mut(),
    );

    let mut viewport_ui = egui::Ui::new(
        ctx.clone(),
        "viewport".into(),
        egui::UiBuilder::new()
            .layer_id(egui::LayerId::background())
            .max_rect(ctx.viewport_rect()),
    );

    egui::Panel::top("top_panel")
        .resizable(false)
        .show(&mut viewport_ui, |ui| {
            egui::MenuBar::new().ui(ui, |ui| {
                file_menu_button(
                    ui,
                    &mut new_chat_group_modal_string_open.1,
                    &mut trpg_group_settings.open,
                );
                tools_menu_button(
                    ui,
                    &mut rule_engine_state,
                    &mut battle_round_state,
                );
                pool_menu_button(ui, &manager, trpg_group_settings);
            });
        });

    egui::Panel::right("right_panel")
        .resizable(true)
        .show(&mut viewport_ui, |ui| {
            if napcat_sender.is_none() {
                ui.label("NapCat websocket未连接");
            }
            if deepseek_sender.is_none() {
                ui.label("DeepSeek后台未就绪");
            }
            let summary_markers_changed =
                sync_summarized_message_counts(&mut manager, &deepseek_manager);
            if summary_markers_changed {
                if let Err(err) = manager.persist() {
                    eprintln!("failed to persist summarized message markers: {err}");
                }
            }

            summary_panel(ui, &manager, &deepseek_manager);
        });

    egui::Panel::left("chat_list_panel")
        .resizable(true)
        .default_size(220.0)
        .size_range(160.0..=340.0)
        .show(&mut viewport_ui, |ui| {
            chat_list_panel(
                ui,
                ctx,
                &mut manager,
                chat_list_edit_target,
                chat_list_edit_name,
                chat_list_player_visible_filter,
                trpg_group_settings,
            );
        });

    egui::CentralPanel::default()
        .frame(egui::Frame::NONE)
        .show(&mut viewport_ui, |ui| {
            let viewport = ui.max_rect();
            let pixels_per_point = ctx.pixels_per_point();

            let toolbar = egui::Frame::new()
                .fill(egui::Color32::from_black_alpha(210))
                .corner_radius(4)
                .inner_margin(6)
                .show(ui, |ui| {
                    let mode_before_toolbar = voxel_editor.mode;
                    ui.horizontal(|ui| {
                        ui.selectable_value(
                            &mut voxel_editor.mode,
                            VoxelEditMode::Add,
                            "添加",
                        );
                        ui.selectable_value(
                            &mut voxel_editor.mode,
                            VoxelEditMode::Remove,
                            "删除",
                        );
                        ui.selectable_value(
                            &mut voxel_editor.mode,
                            VoxelEditMode::Paint,
                            "涂色",
                        );
                        ui.selectable_value(
                            &mut voxel_editor.mode,
                            VoxelEditMode::Physics,
                            "物理选区",
                        );
                        ui.selectable_value(
                            &mut voxel_editor.mode,
                            VoxelEditMode::Drag,
                            "拖拽",
                        );
                        ui.separator();
                        ui.selectable_value(
                            &mut voxel_editor.mode,
                            VoxelEditMode::Push,
                            "推开",
                        );
                        ui.selectable_value(
                            &mut voxel_editor.mode,
                            VoxelEditMode::Pull,
                            "拉近",
                        );
                        ui.selectable_value(
                            &mut voxel_editor.mode,
                            VoxelEditMode::Explode,
                            "爆炸",
                        );
                        ui.separator();
                        for (material, name, color) in voxel_material_choices() {
                            if voxel_material_slot(
                                ui,
                                material,
                                name,
                                color,
                                if voxel_editor.light_tool.is_none() {
                                    voxel_editor.material
                                } else {
                                    0
                                },
                                22.0,
                                None,
                            ) {
                                voxel_editor.select_material(material);
                            }
                        }
                        ui.separator();
                        ui.label("笔刷大小");
                        ui.add(
                            egui::DragValue::new(&mut voxel_editor.brush_radius)
                                .range(0..=MAX_VOXEL_BRUSH_RADIUS),
                        );
                        if ui.button("撤销").clicked() {
                            voxel_editor.undo_requested = true;
                        }
                        if ui.button("重做").clicked() {
                            voxel_editor.redo_requested = true;
                        }
                        if voxel_editor.reset_scene_confirmation() {
                            ui.horizontal(|ui| {
                                if ui
                                    .button("Confirm Reset Scene")
                                    .on_hover_text(
                                        "Delete the saved scene and restore all default voxels, physics props, and lights",
                                    )
                                    .clicked()
                                {
                                    voxel_editor.confirm_reset_scene();
                                }
                                if ui.button("Cancel").clicked() {
                                    voxel_editor.cancel_reset_scene_confirmation();
                                }
                            });
                        } else if ui
                            .button("Reset Scene")
                            .on_hover_text("Requires a second confirmation")
                            .clicked()
                        {
                            voxel_editor.request_reset_scene_confirmation();
                        }
                        if ui.button("保存当前场景").clicked() {
                            voxel_editor.request_scene_snapshot();
                        }
                        if ui.button("视图复位").clicked() {
                            voxel_editor.view_reset_requested = true;
                        }
                        ui.label("移动速度");
                        ui.add(
                            egui::DragValue::new(&mut voxel_editor.first_person_speed)
                                .range(0.25..=50.0)
                                .speed(0.25),
                        );
                        if ui
                            .selectable_label(
                                voxel_editor.creative_inventory_open,
                                "创造物品栏",
                            )
                            .on_hover_text("按 E 打开或关闭；材料无限使用，不计数量")
                            .clicked()
                        {
                            voxel_editor.creative_inventory_open =
                                !voxel_editor.creative_inventory_open;
                        }
                    });
                    if voxel_editor.mode != mode_before_toolbar {
                        let mode = voxel_editor.mode;
                        voxel_editor.select_mode(mode);
                    }
                    if voxel_editor.first_person_enabled {
                        let movement_hint = if voxel_editor.first_person_flying {
                            "创造飞行/穿墙中 · WASD 移动 · 空格上升 · Shift 下降"
                        } else {
                            "WASD 移动 · 空格跳跃 · 双击空格飞行"
                        };
                        let interaction_hint = if voxel_editor.is_player_possession_tool_equipped() {
                            "右键选择/接管玩家 · 右键空处解除"
                        } else if voxel_editor.is_tool_gun_equipped() {
                            "右键发射 · R切换模式"
                        } else {
                            "左键拆除 · 右键放置/使用"
                        };
                        ui.small(format!(
                            "{movement_hint} · 当前工具：{} · {interaction_hint} · 滚轮选择快捷栏 · Esc 释放鼠标 · 点击游戏画面重新锁定",
                            voxel_editor.active_tool_label()
                        ));
                    } else {
                        let interaction_hint = if voxel_editor.is_player_possession_tool_equipped() {
                            "右键选择/接管玩家 · 右键空处解除"
                        } else if voxel_editor.is_tool_gun_equipped() {
                            "右键发射 · R切换模式"
                        } else {
                            "左键拆除 · 右键放置/使用"
                        };
                        ui.small(format!(
                            "当前工具：{} · {interaction_hint} · 中键旋转 · Shift+中键平移",
                            voxel_editor.active_tool_label(),
                        ));
                    }
                    ui.collapsing("光照编辑器", |ui| {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("环境光");
                            ui.add(
                                egui::DragValue::new(&mut voxel_editor.ambient_brightness)
                                    .range(0.0..=500.0)
                                    .speed(1.0),
                            );
                            ui.label("主光");
                            ui.add(
                                egui::DragValue::new(&mut voxel_editor.key_light_illuminance)
                                    .range(0.0..=50_000.0)
                                    .speed(100.0),
                            );
                            ui.label("主光颜色");
                            ui.color_edit_button_rgb(&mut voxel_editor.key_light_color);
                            ui.label("补光");
                            ui.add(
                                egui::DragValue::new(&mut voxel_editor.fill_light_illuminance)
                                    .range(0.0..=50_000.0)
                                    .speed(100.0),
                            );
                            ui.label("补光颜色");
                            ui.color_edit_button_rgb(&mut voxel_editor.fill_light_color);
                            ui.label("辐射级联");
                            ui.add(
                                egui::DragValue::new(&mut voxel_editor.radiance_intensity)
                                    .range(0.0..=3.0)
                                    .speed(0.02),
                            );
                            if ui
                                .button("仅检查级联")
                                .on_hover_text("关闭环境光、主光和补光，只显示体素发光与间接光")
                                .clicked()
                            {
                                voxel_editor.inspect_radiance_lighting();
                            }
                            if ui.button("恢复默认光照").clicked() {
                                voxel_editor.reset_lighting();
                            }
                        });
                    });
                    if voxel_editor.light_tool == Some(VoxelLightTool::Edit) {
                        egui::CollapsingHeader::new("灯光编辑工具")
                            .default_open(true)
                            .show(ui, |ui| {
                                if voxel_editor.has_selected_light() {
                                    ui.horizontal_wrapped(|ui| {
                                        ui.label("颜色");
                                        ui.color_edit_button_rgb(
                                            &mut voxel_editor.placed_light_color,
                                        );
                                        ui.label("亮度");
                                        ui.add(
                                            egui::DragValue::new(
                                                &mut voxel_editor.placed_light_intensity,
                                            )
                                            .range(0.0..=100_000.0)
                                            .speed(100.0),
                                        );
                                        ui.label("范围");
                                        ui.add(
                                            egui::DragValue::new(
                                                &mut voxel_editor.placed_light_range,
                                            )
                                            .range(0.25..=100.0)
                                            .speed(0.25),
                                        );
                                    });
                                } else {
                                    ui.small("先用右键选择场景中的点光、方块灯、聚光灯或物理灯");
                                }
                            });
                    }
                    let snapshot_labels = voxel_editor.scene_snapshot_labels();
                    ui.collapsing(
                        format!("场景历史（{}）", snapshot_labels.len()),
                        |ui| {
                            if snapshot_labels.is_empty() {
                                ui.small("还没有场景快照");
                            }
                            for (index, label) in snapshot_labels.iter().enumerate().rev() {
                                if ui
                                    .button(label)
                                    .on_hover_text("点击恢复体素和物理体到这个状态")
                                    .clicked()
                                {
                                    voxel_editor.request_scene_restore(index);
                                }
                            }
                        },
                    );
                    if voxel_editor.mode == VoxelEditMode::Physics {
                        ui.horizontal_wrapped(|ui| {
                            ui.label(voxel_editor.physics_selection_hint());
                            if ui
                                .add_enabled(
                                    voxel_editor.has_physics_selection(),
                                    egui::Button::new("生成物理体"),
                                )
                                .on_hover_text("选区内所有固体方块会合并为一个动态物理体")
                                .clicked()
                            {
                                voxel_editor.physics_requested = true;
                            }
                            if let Some(status) = voxel_editor.physics_status() {
                                ui.small(status);
                            }
                        });
                    }
                    if matches!(
                        voxel_editor.mode,
                        VoxelEditMode::Push | VoxelEditMode::Pull
                    ) {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("推拉冲量");
                            ui.add(
                                egui::DragValue::new(&mut voxel_editor.physics_push_pull_impulse)
                                    .range(0.1..=200.0)
                                    .speed(0.25),
                            );
                            ui.label("右键点击方块或物理体；静态方块会按笔刷大小自动物理化");
                            if let Some(status) = voxel_editor.physics_status() {
                                ui.small(status);
                            }
                        });
                    }
                    if voxel_editor.mode == VoxelEditMode::Explode {
                        ui.horizontal_wrapped(|ui| {
                            ui.label("爆炸冲量");
                            ui.add(
                                egui::DragValue::new(&mut voxel_editor.physics_explosion_impulse)
                                    .range(0.1..=500.0)
                                    .speed(0.5),
                            );
                            ui.label("半径");
                            ui.add(
                                egui::DragValue::new(&mut voxel_editor.physics_explosion_radius)
                                    .range(0.25..=100.0)
                                    .speed(0.25),
                            );
                            ui.label("右键点击爆心；每次爆炸最多新建60个物理碎块，超额方块会合并");
                            if let Some(status) = voxel_editor.physics_status() {
                                ui.small(status);
                            }
                        });
                    }
                });
            voxel_editor.set_viewport_bounds(
                bevy::prelude::Vec2::new(
                    viewport.min.x * pixels_per_point,
                    viewport.min.y * pixels_per_point,
                ),
                bevy::prelude::Vec2::new(
                    viewport.max.x * pixels_per_point,
                    viewport.max.y * pixels_per_point,
                ),
                toolbar.response.rect.max.y * pixels_per_point,
            );

            if let Some(possessed_user_id) = voxel_possession.active_user_id {
                voxel_editor.creative_inventory_open = false;
                egui::Area::new(egui::Id::new("voxel_player_hotbar"))
                    .anchor(egui::Align2::CENTER_BOTTOM, egui::vec2(0.0, -12.0))
                    .order(egui::Order::Foreground)
                    .show(ctx, |ui| {
                        egui::Frame::new()
                            .fill(egui::Color32::from_black_alpha(210))
                            .corner_radius(4)
                            .inner_margin(4)
                            .show(ui, |ui| {
                                if let Some(character) = manager
                                    .player_characters
                                    .get(&possessed_user_id.to_string())
                                {
                                    ui.horizontal(|ui| {
                                        for (slot, entry) in
                                            character.inventory.hotbar.iter().enumerate()
                                        {
                                            let full_label =
                                                character_hotbar_slot_label(*entry, character);
                                            let short_label =
                                                character_hotbar_slot_short_label(*entry, character);
                                            let selected =
                                                voxel_possession.selected_hotbar_slot == slot;
                                            let response = ui.add_sized(
                                                [62.0, 44.0],
                                                egui::Button::new(format!(
                                                    "{}\n{}",
                                                    slot + 1,
                                                    short_label
                                                ))
                                                .selected(selected),
                                            );
                                            if response.on_hover_text(full_label).clicked() {
                                                voxel_possession.selected_hotbar_slot = slot;
                                            }
                                        }
                                    });
                                } else {
                                    ui.label("该玩家还没有角色物品栏");
                                }
                            });
                    });
                if voxel_possession.player_inventory_open {
                    let target_id = possessed_user_id.to_string();
                    let skill_pool_snapshot = manager.skill_pool.clone();
                    let item_pool_snapshot = manager.item_pool.clone();
                    let stat_config = manager.character_stat_config_for_target(&target_id);
                    let mut window_open = true;
                    let mut inventory_changed = false;
                    let mut equipment_changed = false;
                    if let Some(character) = manager.player_characters.get_mut(&target_id) {
                        let title = format!(
                            "玩家背包 · {}",
                            if character.name.trim().is_empty() {
                                target_id.as_str()
                            } else {
                                character.name.trim()
                            }
                        );
                        egui::Window::new(title)
                            .id(egui::Id::new(("voxel_player_inventory", possessed_user_id)))
                            .default_width(620.0)
                            .resizable(true)
                            .open(&mut window_open)
                            .show(ctx, |ui| {
                                ui.small("E关闭 · GM可编辑装备、1-9快捷栏和背包物品");
                                let result = ui
                                    .push_id(("possessed_inventory", possessed_user_id), |ui| {
                                        character_inventory_editor_ui(
                                            ui,
                                            &target_id,
                                            character,
                                            character_edit_state,
                                            &skill_pool_snapshot,
                                            &item_pool_snapshot,
                                            true,
                                        )
                                    })
                                    .inner;
                                inventory_changed |= result.0;
                                equipment_changed |= result.1;
                            });
                        if equipment_changed {
                            sync_character_buffs(
                                &target_id,
                                character,
                                &stat_config,
                                &mut rule_engine_state,
                                &skill_pool_snapshot,
                            );
                        }
                    } else {
                        window_open = false;
                    }
                    voxel_possession.player_inventory_open = window_open;
                    if inventory_changed {
                        manager.persist().ok();
                    }
                }
            } else {
            let mut hotbar_drop = None;
            let mut hotbar_clicked = None;
            let mut hotbar_delete = None;
            let hotbar_area = egui::Area::new(egui::Id::new("voxel_creative_hotbar"))
                .anchor(
                    egui::Align2::CENTER_BOTTOM,
                    egui::vec2(0.0, -12.0),
                )
                .order(egui::Order::Foreground)
                .show(ctx, |ui| {
                    egui::Frame::new()
                        .fill(egui::Color32::from_black_alpha(210))
                        .corner_radius(4)
                        .inner_margin(4)
                        .show(ui, |ui| {
                            ui.horizontal(|ui| {
                                for slot in 0..voxel_editor.creative_hotbar.len() {
                                    let shortcut = if slot == 9 {
                                        "0".to_owned()
                                    } else {
                                        (slot + 1).to_string()
                                    };
                                    let (drop_zone, dropped) = ui
                                        .dnd_drop_zone::<VoxelCreativeDragPayload, _>(
                                            egui::Frame::new(),
                                            |ui| {
                                                if let Some(item) =
                                                    voxel_editor.creative_hotbar[slot]
                                                {
                                                    voxel_creative_drag_source(
                                                        ui,
                                                        egui::Id::new(("voxel_hotbar_drag", slot)),
                                                        VoxelCreativeDragPayload::Hotbar { slot },
                                                        item,
                                                        voxel_editor.selected_hotbar_slot == slot,
                                                        38.0,
                                                        Some(&shortcut),
                                                    )
                                                } else {
                                                    voxel_creative_item_slot(
                                                        ui,
                                                        None,
                                                        voxel_editor.selected_hotbar_slot == slot,
                                                        38.0,
                                                        Some(&shortcut),
                                                    )
                                                }
                                            },
                                        );
                                    let response = drop_zone.inner;
                                    if response.clicked() {
                                        hotbar_clicked = Some(slot);
                                    }
                                    if response.secondary_clicked() {
                                        hotbar_delete = Some(slot);
                                    }
                                    if let Some(payload) = dropped {
                                        hotbar_drop = Some((slot, *payload));
                                    }
                                }
                            });
                        });
                });
            let hotbar_rect = hotbar_area.response.rect;

            let mut inventory_rect = None;
            if voxel_editor.creative_inventory_open {
                const CREATIVE_INVENTORY_WIDTH: f32 = 440.0;
                let mut picked_item = None;
                let mut delete_selected = false;
                let mut window_open = true;
                let inventory_window = egui::Window::new("创造模式物品栏")
                    .id(egui::Id::new(
                        "voxel_creative_inventory_v2",
                    ))
                    .anchor(
                        egui::Align2::CENTER_CENTER,
                        egui::Vec2::ZERO,
                    )
                    .collapsible(false)
                    .default_width(CREATIVE_INVENTORY_WIDTH)
                    .min_width(CREATIVE_INVENTORY_WIDTH)
                    .max_width(CREATIVE_INVENTORY_WIDTH)
                    .resizable(false)
                    .open(&mut window_open)
                    .show(ctx, |ui| {
                        ui.set_max_width(CREATIVE_INVENTORY_WIDTH);
                        ui.label("浏览全部物品 · 拖入快捷栏 · 拖出物品栏删除 · 创造模式无限数量");
                        ui.horizontal(|ui| {
                            ui.label(format!(
                                "当前快捷栏：{}",
                                if voxel_editor.selected_hotbar_slot == 9 {
                                    "0".to_owned()
                                } else {
                                    (voxel_editor.selected_hotbar_slot + 1).to_string()
                                }
                            ));
                            if ui.button("删除当前格物品").clicked() {
                                delete_selected = true;
                            }
                        });
                        ui.add_space(6.0);
                        ui.strong("建筑方块");
                        egui::Grid::new("voxel_creative_inventory_grid_v2")
                            .min_col_width(56.0)
                            .max_col_width(72.0)
                            .spacing(egui::vec2(10.0, 10.0))
                            .show(ui, |ui| {
                                for (index, (material, name, _color)) in
                                    voxel_material_choices().into_iter().enumerate()
                                {
                                    ui.vertical_centered(|ui| {
                                        let item = VoxelCreativeItem::Material(material);
                                        if voxel_creative_drag_source(
                                            ui,
                                            egui::Id::new(("voxel_catalog_material", material)),
                                            VoxelCreativeDragPayload::Catalog(item),
                                            item,
                                            voxel_editor.creative_hotbar
                                                [voxel_editor.selected_hotbar_slot]
                                                == Some(item),
                                            56.0,
                                            None,
                                        )
                                        .clicked()
                                        {
                                            picked_item = Some(item);
                                        }
                                        ui.small(name);
                                    });
                                    if (index + 1) % 5 == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                        ui.separator();
                        ui.strong("灯光与工具");
                        egui::Grid::new("voxel_creative_light_inventory_grid_v2")
                            .min_col_width(56.0)
                            .max_col_width(72.0)
                            .spacing(egui::vec2(10.0, 10.0))
                            .show(ui, |ui| {
                                for (index, tool) in VoxelLightTool::ALL.into_iter().enumerate() {
                                    let item = VoxelCreativeItem::Light(tool);
                                    let (name, _) = voxel_creative_item_visual(item);
                                    ui.vertical_centered(|ui| {
                                        if voxel_creative_drag_source(
                                            ui,
                                            egui::Id::new(("voxel_catalog_light", index)),
                                            VoxelCreativeDragPayload::Catalog(item),
                                            item,
                                            voxel_editor.creative_hotbar
                                                [voxel_editor.selected_hotbar_slot]
                                                == Some(item),
                                            56.0,
                                            None,
                                        )
                                        .on_hover_text(match tool {
                                            VoxelLightTool::Point => "暖色全向点光源",
                                            VoxelLightTool::DarkPoint => "低亮度暗紫色点光源",
                                            VoxelLightTool::Cube => "发光体素方块灯",
                                            VoxelLightTool::Spot => "沿放置表面朝外的聚光灯",
                                            VoxelLightTool::Physics => "可掉落、碰撞和移动的实体灯",
                                            VoxelLightTool::Edit => {
                                                "选择场景灯光并编辑颜色、亮度和范围"
                                            },
                                            VoxelLightTool::Remove => "瞄准已放置灯光并移除",
                                        })
                                        .clicked()
                                        {
                                            picked_item = Some(item);
                                        }
                                        ui.small(name);
                                    });
                                    if (index + 1) % 3 == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                        ui.separator();
                        ui.strong("编辑工具");
                        egui::Grid::new("voxel_creative_mode_inventory_grid_v2")
                            .min_col_width(48.0)
                            .max_col_width(72.0)
                            .spacing(egui::vec2(10.0, 10.0))
                            .show(ui, |ui| {
                                let possession_tool = VoxelCreativeItem::PlayerPossessionTool;
                                let (name, _) = voxel_creative_item_visual(possession_tool);
                                ui.vertical_centered(|ui| {
                                    if voxel_creative_drag_source(
                                        ui,
                                        egui::Id::new("voxel_catalog_player_possession_tool"),
                                        VoxelCreativeDragPayload::Catalog(possession_tool),
                                        possession_tool,
                                        voxel_editor.creative_hotbar
                                            [voxel_editor.selected_hotbar_slot]
                                            == Some(possession_tool),
                                        48.0,
                                        None,
                                    )
                                    .on_hover_text(
                                        "GM右键玩家立绘即可选择并接管；右键空处解除接管",
                                    )
                                    .clicked()
                                    {
                                        picked_item = Some(possession_tool);
                                    }
                                    ui.small(name);
                                });
                                let teleport_tool = VoxelCreativeItem::TeleportTool;
                                let (name, _) = voxel_creative_item_visual(teleport_tool);
                                ui.vertical_centered(|ui| {
                                    if voxel_creative_drag_source(
                                        ui,
                                        egui::Id::new("voxel_catalog_teleport_tool"),
                                        VoxelCreativeDragPayload::Catalog(teleport_tool),
                                        teleport_tool,
                                        voxel_editor.creative_hotbar
                                            [voxel_editor.selected_hotbar_slot]
                                            == Some(teleport_tool),
                                        48.0,
                                        None,
                                    )
                                    .on_hover_text("GM右键打开目的地列表，可传送到空间站、战斗舰或行星科研站")
                                    .clicked()
                                    {
                                        picked_item = Some(teleport_tool);
                                    }
                                    ui.small(name);
                                });
                                let tool_gun = VoxelCreativeItem::ToolGun;
                                let (name, _) = voxel_creative_item_visual(tool_gun);
                                ui.vertical_centered(|ui| {
                                    if voxel_creative_drag_source(
                                        ui,
                                        egui::Id::new("voxel_catalog_tool_gun"),
                                        VoxelCreativeDragPayload::Catalog(tool_gun),
                                        tool_gun,
                                        voxel_editor.creative_hotbar
                                            [voxel_editor.selected_hotbar_slot]
                                            == Some(tool_gun),
                                        48.0,
                                        None,
                                    )
                                    .on_hover_text("GMod风格工具枪：右键使用，R切换物理化、拖拽、推开、拉近和爆炸模式")
                                    .clicked()
                                    {
                                        picked_item = Some(tool_gun);
                                    }
                                    ui.small(name);
                                });
                                for (index, mode) in VoxelEditMode::ALL.into_iter().enumerate() {
                                    let item = VoxelCreativeItem::Mode(mode);
                                    let (name, _) = voxel_creative_item_visual(item);
                                    ui.vertical_centered(|ui| {
                                        if voxel_creative_drag_source(
                                            ui,
                                            egui::Id::new(("voxel_catalog_mode", index)),
                                            VoxelCreativeDragPayload::Catalog(item),
                                            item,
                                            voxel_editor.creative_hotbar
                                                [voxel_editor.selected_hotbar_slot]
                                                == Some(item),
                                            48.0,
                                            None,
                                        )
                                        .clicked()
                                        {
                                            picked_item = Some(item);
                                        }
                                        ui.small(name);
                                    });
                                    if (index + 4) % 4 == 0 {
                                        ui.end_row();
                                    }
                                }
                            });
                        if let Some(status) = voxel_editor.physics_status() {
                            ui.small(status);
                        }
                        ui.add_space(4.0);
                        ui.small("拖动物品到任意快捷栏格 · 快捷栏内拖动可换位 · 拖到物品栏外删除");
                    });
                inventory_rect = inventory_window
                    .as_ref()
                    .map(|response| response.response.rect);
                if delete_selected {
                    let slot = voxel_editor.selected_hotbar_slot;
                    voxel_editor.delete_hotbar_slot(slot);
                }
                if let Some(item) = picked_item {
                    voxel_editor.put_in_selected_hotbar(item);
                }
                if !window_open {
                    voxel_editor.creative_inventory_open = false;
                }
            }

            if voxel_editor.teleport_menu_open {
                let mut window_open = true;
                egui::Window::new("传送器")
                    .id(egui::Id::new("voxel_teleport_tool_window"))
                    .anchor(
                        egui::Align2::CENTER_CENTER,
                        egui::Vec2::ZERO,
                    )
                    .collapsible(false)
                    .resizable(false)
                    .open(&mut window_open)
                    .show(ctx, |ui| {
                        ui.label("选择目的地");
                        ui.small("传送只移动GM；被接管的生存玩家不会受到影响。");
                        ui.separator();
                        for destination in VoxelTeleportDestination::ALL {
                            if ui
                                .add_sized(
                                    egui::vec2(220.0, 28.0),
                                    egui::Button::new(destination.label()),
                                )
                                .clicked()
                            {
                                voxel_editor.request_teleport(destination);
                            }
                        }
                        let mut standee_destinations = player_standees
                            .iter()
                            .filter(|(_, visibility)| {
                                **visibility != bevy::prelude::Visibility::Hidden
                            })
                            .map(|(standee, _)| {
                                let target_id = standee.user_id.to_string();
                                let character_name = manager
                                    .player_characters
                                    .get(&target_id)
                                    .map(|character| {
                                        if !character.nickname.trim().is_empty() {
                                            character.nickname.trim()
                                        } else {
                                            character.name.trim()
                                        }
                                    })
                                    .filter(|name| !name.is_empty())
                                    .map(str::to_owned)
                                    .unwrap_or_else(|| target_display_name(&manager, &target_id));
                                (character_name, standee.user_id)
                            })
                            .collect::<Vec<_>>();
                        standee_destinations.sort_by(|left, right| {
                            left.0.cmp(&right.0).then_with(|| left.1.cmp(&right.1))
                        });
                        if !standee_destinations.is_empty() {
                            ui.separator();
                            ui.strong("玩家立牌");
                            for (name, user_id) in standee_destinations {
                                if ui
                                    .add_sized(
                                        egui::vec2(220.0, 28.0),
                                        egui::Button::new(format!("传送到 {name}")),
                                    )
                                    .on_hover_text(format!("QQ：{user_id}"))
                                    .clicked()
                                {
                                    voxel_editor.request_teleport(
                                        VoxelTeleportDestination::PlayerStandee(user_id),
                                    );
                                }
                            }
                        }
                    });
                if !window_open {
                    voxel_editor.teleport_menu_open = false;
                }
            }

            if let Some(slot) = hotbar_clicked {
                voxel_editor.select_hotbar_slot(slot);
            }
            if let Some(slot) = hotbar_delete {
                voxel_editor.delete_hotbar_slot(slot);
            }
            if let Some((destination, payload)) = hotbar_drop {
                match payload {
                    VoxelCreativeDragPayload::Catalog(item) => {
                        voxel_editor.selected_hotbar_slot = destination;
                        voxel_editor.put_in_selected_hotbar(item);
                    },
                    VoxelCreativeDragPayload::Hotbar { slot } => {
                        voxel_editor.swap_hotbar_slots(slot, destination);
                    },
                }
            } else if ctx.input(|input| input.pointer.any_released()) {
                if let Some(payload) = egui::DragAndDrop::payload::<VoxelCreativeDragPayload>(ctx) {
                    if let VoxelCreativeDragPayload::Hotbar { slot } = *payload {
                        let pointer = ctx.pointer_interact_pos();
                        let outside_hotbar =
                            pointer.is_none_or(|position| !hotbar_rect.contains(position));
                        let outside_inventory = inventory_rect.is_none_or(|rect| {
                            pointer.is_none_or(|position| !rect.contains(position))
                        });
                        if outside_hotbar && outside_inventory {
                            voxel_editor.delete_hotbar_slot(slot);
                        }
                    }
                }
            }
            }

            if voxel_editor.first_person_enabled
                && !voxel_editor.creative_inventory_open
                && !voxel_editor.teleport_menu_open
                && !voxel_possession.player_inventory_open
            {
                let center = viewport.center();
                let painter = ui.painter();
                let stroke = Stroke::new(2.0, egui::Color32::WHITE);
                painter.line_segment(
                    [
                        center + egui::vec2(-7.0, 0.0),
                        center + egui::vec2(7.0, 0.0),
                    ],
                    stroke,
                );
                painter.line_segment(
                    [
                        center + egui::vec2(0.0, -7.0),
                        center + egui::vec2(0.0, 7.0),
                    ],
                    stroke,
                );
            }

            pending_chat_requests_window(
                ctx,
                &mut manager,
                napcat_sender,
                &mut ime,
            );
            waiting_turn_manager_window(ctx, &mut manager);

            let mut closed_group_names = Vec::new();
            for (k, v) in &manager.groups.clone() {
                let group_title = chat_group_title(&k, v, &manager);
                let unread_count = chat_group_unread_count(&manager, v);
                let group_size = group_chat_inner_size(v.members.len(), ui.max_rect());
                let group_max_size = group_chat_max_size(ui.max_rect());
                let mut group_open = true;
                let response = egui::Window::new(group_title)
                    .open(&mut group_open)
                    .constrain_to(ui.max_rect())
                    .id(Id::new((
                        k.as_str(),
                        "chat_group_window_v2",
                    )))
                    .default_pos(ui.max_rect().left_top() + egui::vec2(12.0, 12.0))
                    .default_size(group_size)
                    .min_size(CHAT_WINDOW_MIN_SIZE)
                    .max_size(group_max_size)
                    .show(ctx, |ui| {
                        group_drop_area_ui(ui, &k, &v.members);
                        group_broadcast_input_ui(
                            ui,
                            ctx,
                            &k,
                            &v.members,
                            &manager,
                            napcat_sender,
                            chat_input_msgs,
                            group_broadcast_scopes,
                            &mut ime,
                        );
                    });

                if !group_open {
                    closed_group_names.push(k.clone());
                    continue;
                }

                if let Some(response) = response {
                    paint_unread_badge(
                        ctx,
                        response.response.rect,
                        unread_count,
                    );
                    if response.inner.is_some() {
                        if let Some(previous_rect) = previous_group_rects.get(k) {
                            let delta = response.response.rect.min - previous_rect.min;
                            if delta.length_sq() > 0.0 {
                                group_deltas.insert(k.clone(), delta);
                            }
                        }
                        latest_group_rects.insert(k.clone(), response.response.rect);
                        group_rects.insert(k.clone(), response.response.rect);
                    }
                }
            }
            if !closed_group_names.is_empty() {
                for group_name in &closed_group_names {
                    manager.groups.remove(group_name);
                    previous_group_rects.remove(group_name);
                }
                if let Err(err) = manager.persist() {
                    eprintln!("failed to persist closed chat groups: {err}");
                }
            }
            **previous_group_rects = latest_group_rects;

            let mut visible_targets: HashSet<String> = manager.open_chat_targets.clone();
            for group in manager.groups.values() {
                visible_targets.extend(group.members.iter().cloned());
            }

            for target_id in visible_targets {
                let messages = manager
                    .messages
                    .get(&target_id)
                    .cloned()
                    .unwrap_or_default();
                let id = egui::Id::new(&target_id);
                let mut default_rect: Rect = Rect::from_pos(Pos2::new(0.0, 0.0));
                if !**has_run_once {
                    ctx.memory(|m| {
                        if let Some(rect) = m.area_rect(id) {
                            default_rect = rect;
                        }
                    });
                    **has_run_once = true
                }

                let current_group = if manager.open_chat_targets.contains(&target_id) {
                    None
                } else {
                    manager.groups.iter().find_map(|(group_name, group)| {
                        group
                            .members
                            .contains(&target_id)
                            .then_some(group_name.clone())
                    })
                };
                let rect = if let Some(group_name) = current_group.as_deref() {
                    let Some(rect) = group_rects.get(group_name).copied() else {
                        continue;
                    };
                    rect
                } else {
                    ui.max_rect()
                };
                let (_nickname, heights) = get_nickname_lens(target_id.clone(), &messages);
                let window_title = target_display_name(&manager, &target_id);
                let targets = targets_for_target(&manager, &target_id);
                let unread_count = target_unread_count(&manager, &target_id);
                let summary_request_changed = queue_summaries_if_needed(
                    &manager,
                    &target_id,
                    &messages,
                    &manager.summarized_message_counts,
                    deepseek_sender,
                    &mut deepseek_manager,
                );
                if summary_request_changed {
                    if let Err(err) = deepseek_manager.persist() {
                        eprintln!("failed to persist DeepSeek summary request: {err}");
                    }
                }

                let active_trpg_group = manager.current_trpg_group.clone();
                chat_window(
                    &window_title,
                    id,
                    rect,
                    ctx,
                    heights,
                    &messages,
                    napcat_sender,
                    &target_id,
                    chat_input_msgs,
                    targets,
                    &mut ime,
                    chat_scroll_states,
                    &group_rects,
                    &mut manager,
                    current_group.as_deref(),
                    current_group
                        .as_deref()
                        .and_then(|group_name| group_deltas.get(group_name).copied()),
                    unread_count,
                    quick_character_targets,
                    image_textures,
                    active_trpg_group.as_deref(),
                    turn_count_drafts,
                    chat_player_visible_previews,
                    &mut rule_engine_state,
                    player_view_request.as_deref_mut(),
                );
            }
        });

    let should_persist_ui_memory = ctx.input(|input| {
        input.pointer.any_released()
            || input.events.iter().any(|event| {
                matches!(
                    event,
                    egui::Event::Copy
                        | egui::Event::Cut
                        | egui::Event::Paste(_)
                        | egui::Event::Text(_)
                        | egui::Event::Key { .. }
                )
            })
    });
    ctx.memory(|m| {
        cached_memory.ui_memory = m.clone();
    });
    if should_persist_ui_memory {
        cached_memory.persist().ok();
    }
}

fn targets_for_target(manager: &NapcatMessageManager, target_id: &str) -> Vec<NapcatSendTarget> {
    let kind = manager.chat_target_kind(target_id);
    let Ok(target_id) = target_id.parse::<u64>() else {
        eprintln!("invalid NapCat target id: {target_id}");
        return Vec::new();
    };

    match kind {
        ChatTargetExportKind::Group => vec![NapcatSendTarget::Group(target_id)],
        ChatTargetExportKind::Private | ChatTargetExportKind::Unknown => {
            vec![NapcatSendTarget::Private(target_id)]
        },
    }
}

fn append_local_sent_message(
    manager: &mut NapcatMessageManager,
    target: NapcatSendTarget,
    text: &str,
) -> bool {
    let (target_id, message_type, group_id, recipient_id) = match target {
        NapcatSendTarget::Private(user_id) => (
            user_id.to_string(),
            NapcatMessageType::Private,
            None,
            Some(user_id),
        ),
        NapcatSendTarget::Group(group_id) => (
            group_id.to_string(),
            NapcatMessageType::Group,
            Some(group_id),
            None,
        ),
    };

    let self_id = manager
        .messages
        .get(&target_id)
        .and_then(|messages| messages.first())
        .or_else(|| {
            manager
                .messages
                .values()
                .find_map(|messages| messages.first())
        })
        .map(|message| message.data.self_id)
        .unwrap_or_default();
    let time = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_secs())
        .unwrap_or_default();
    let mut message = NapcatMessage {
        data: NapcatMessageData {
            time,
            message_type,
            message: vec![NapcatMessageChain {
                variant: NapcatMessageChainType::Text {
                    data: TextData {
                        text: text.to_owned(),
                    },
                },
            }],
            self_id,
            user_id: self_id,
            group_id,
            group_name: None,
            target_id: recipient_id,
            sender: NapcatSender {
                user_id: self_id,
                nickname: "GM".to_owned(),
            },
            campaign_id: String::new(),
            character_id: None,
            party_id: None,
            visibility: Visibility::Public,
            access_scope_resolved: false,
        },
    };
    manager.annotate_message_access(&target_id, &mut message);

    manager.messages.entry(target_id).or_default().push(message);
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn repeated_character_skill_rows_have_distinct_widget_ids() {
        assert_ne!(
            character_skill_ui_id("player", 0),
            character_skill_ui_id("player", 1)
        );
        assert_ne!(
            character_skill_ui_id("player", 0),
            character_skill_ui_id("other-player", 0)
        );
    }

    fn empty_manager() -> NapcatMessageManager {
        NapcatMessageManager {
            messages: HashMap::default(),
            chat_targets: HashMap::default(),
            chat_target_kinds: HashMap::default(),
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
            item_pool: Vec::new(),
            unit_pool: HashMap::default(),
        }
    }

    fn test_private_message(user_id: u64) -> NapcatMessage {
        NapcatMessage {
            data: NapcatMessageData {
                time: 1780132600,
                message_type: NapcatMessageType::Private,
                message: vec![NapcatMessageChain {
                    variant: NapcatMessageChainType::Text {
                        data: TextData {
                            text: "hello".to_owned(),
                        },
                    },
                }],
                self_id: 1,
                user_id,
                group_id: None,
                group_name: None,
                target_id: None,
                sender: NapcatSender {
                    user_id,
                    nickname: format!("user-{user_id}"),
                },
                campaign_id: "default".to_owned(),
                character_id: None,
                party_id: None,
                visibility: Visibility::Public,
                access_scope_resolved: false,
            },
        }
    }

    fn test_group_message(user_id: u64, text: &str) -> NapcatMessage {
        NapcatMessage {
            data: NapcatMessageData {
                time: 1780132600,
                message_type: NapcatMessageType::Group,
                message: vec![NapcatMessageChain {
                    variant: NapcatMessageChainType::Text {
                        data: TextData {
                            text: text.to_owned(),
                        },
                    },
                }],
                self_id: 1,
                user_id,
                group_id: Some(99),
                group_name: Some("测试群".to_owned()),
                target_id: None,
                sender: NapcatSender {
                    user_id,
                    nickname: format!("user-{user_id}"),
                },
                campaign_id: "default".to_owned(),
                character_id: None,
                party_id: None,
                visibility: Visibility::Public,
                access_scope_resolved: false,
            },
        }
    }

    fn split_party_summary_manager() -> NapcatMessageManager {
        let mut manager = empty_manager();
        let mut group = TrpgGroup {
            players: vec!["2".to_owned(), "3".to_owned()],
            group_chats: vec!["99".to_owned()],
            ..Default::default()
        };
        group.ensure_party("red");
        group.ensure_party("blue");
        group.set_player_party("2", Some("red"));
        group.set_player_party("3", Some("blue"));
        manager.trpg_groups.insert("table".to_owned(), group);
        manager.current_trpg_group = Some("table".to_owned());
        manager
    }

    fn manager_with_noncurrent_beta_targets() -> NapcatMessageManager {
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
        manager.messages.insert("2".to_owned(), vec![
            test_private_message(2),
        ]);
        manager.messages.insert("99".to_owned(), vec![
            test_group_message(2, "hello group"),
        ]);
        manager
    }

    #[test]
    fn local_private_send_uses_noncurrent_target_campaign() {
        let mut manager = manager_with_noncurrent_beta_targets();

        assert!(append_local_sent_message(
            &mut manager,
            NapcatSendTarget::Private(2),
            "private answer",
        ));

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
    fn local_group_send_uses_noncurrent_target_campaign() {
        let mut manager = manager_with_noncurrent_beta_targets();

        assert!(append_local_sent_message(
            &mut manager,
            NapcatSendTarget::Group(99),
            "group answer",
        ));

        let response = manager.messages["99"].last().unwrap();
        assert_eq!(response.data.campaign_id, "campaign-b");
        assert_eq!(response.data.character_id, None);
        assert_eq!(response.data.party_id, None);
        assert_eq!(
            response.data.visibility,
            Visibility::Public
        );
    }

    #[test]
    fn history_free_imported_targets_are_listed_and_routed_by_persisted_kind() {
        let mut manager = empty_manager();
        for (target_id, kind) in [
            ("2", ChatTargetExportKind::Private),
            ("99", ChatTargetExportKind::Group),
        ] {
            manager
                .chat_targets
                .insert(target_id.to_owned(), Default::default());
            manager.chat_target_kinds.insert(target_id.to_owned(), kind);
        }

        let views = chat_list_target_views(&manager, None);
        assert_eq!(
            views
                .iter()
                .map(|view| (
                    view.target_id.as_str(),
                    view.message_count
                ))
                .collect::<Vec<_>>(),
            vec![("2", 0), ("99", 0)]
        );
        assert_eq!(targets_for_target(&manager, "2"), vec![
            NapcatSendTarget::Private(2)
        ]);
        assert_eq!(
            targets_for_target(&manager, "99"),
            vec![NapcatSendTarget::Group(99)]
        );
        assert_eq!(
            sorted_pool_targets(&manager, false),
            vec!["2".to_owned()]
        );
        assert_eq!(
            sorted_pool_targets(&manager, true),
            vec!["99".to_owned()]
        );
    }

    #[test]
    fn acknowledged_send_creates_history_for_history_free_imported_target() {
        let mut manager = empty_manager();
        manager
            .chat_targets
            .insert("2".to_owned(), Default::default());
        manager.chat_target_kinds.insert(
            "2".to_owned(),
            ChatTargetExportKind::Private,
        );

        assert!(append_local_sent_message(
            &mut manager,
            NapcatSendTarget::Private(2),
            "first message",
        ));

        let message = manager.messages["2"].first().unwrap();
        assert_eq!(message.data.self_id, 0);
        assert_eq!(message.data.user_id, 0);
        assert_eq!(message.data.target_id, Some(2));
        assert_eq!(
            manager.chat_target_kind("2"),
            ChatTargetExportKind::Private
        );
    }

    #[test]
    fn chat_list_player_filter_hides_inaccessible_targets_and_unread_activity() {
        let mut manager = split_party_summary_manager();
        manager
            .trpg_groups
            .get_mut("table")
            .unwrap()
            .group_chats
            .push("98".to_owned());

        let mut red_message = test_group_message(2, "red clue");
        red_message.data.time = 10;
        let mut blue_message = test_group_message(3, "blue clue");
        blue_message.data.time = 20;
        let mut public_message = test_group_message(4, "public clue");
        public_message.data.time = 30;
        manager.messages.insert("99".to_owned(), vec![
            red_message,
            blue_message,
            public_message,
        ]);
        manager.messages.insert("98".to_owned(), vec![
            test_group_message(3, "blue only"),
        ]);
        manager.messages.insert("2".to_owned(), vec![
            test_private_message(2),
        ]);
        manager.messages.insert("3".to_owned(), vec![
            test_private_message(3),
        ]);
        manager.read_message_counts.insert("99".to_owned(), 1);

        assert_eq!(
            chat_list_player_filter_options(&manager),
            vec!["2".to_owned(), "3".to_owned()]
        );

        let red_views = chat_list_target_views(&manager, Some(2));
        let red_target_ids = red_views
            .iter()
            .map(|view| view.target_id.as_str())
            .collect::<Vec<_>>();

        assert!(red_target_ids.contains(&"2"));
        assert!(red_target_ids.contains(&"99"));
        assert!(!red_target_ids.contains(&"3"));
        assert!(!red_target_ids.contains(&"98"));

        let group_view = red_views
            .iter()
            .find(|view| view.target_id == "99")
            .unwrap();
        assert_eq!(group_view.message_count, 2);
        assert_eq!(group_view.total_message_count, 3);
        assert_eq!(group_view.unread_count, 1);

        let raw_views = chat_list_target_views(&manager, None);
        let raw_target_ids = raw_views
            .iter()
            .map(|view| view.target_id.as_str())
            .collect::<Vec<_>>();
        assert!(raw_target_ids.contains(&"3"));
        assert!(raw_target_ids.contains(&"98"));
    }

    #[test]
    fn chat_list_player_filter_excludes_other_campaign_unread_activity() {
        let mut manager = split_party_summary_manager();
        let current_message = test_group_message(2, "current campaign");
        let mut other_campaign_message = test_group_message(2, "hidden campaign");
        other_campaign_message.data.campaign_id = "other".to_owned();
        manager.messages.insert("99".to_owned(), vec![
            current_message,
            other_campaign_message,
        ]);
        manager.read_message_counts.insert("99".to_owned(), 1);

        let group_view = chat_list_target_views(&manager, Some(2))
            .into_iter()
            .find(|view| view.target_id == "99")
            .unwrap();

        assert_eq!(group_view.message_count, 1);
        assert_eq!(group_view.total_message_count, 2);
        assert_eq!(group_view.unread_count, 0);
    }

    fn buff(name: &str, turns_remaining: i32) -> BuffSpec {
        BuffSpec {
            name: name.to_owned(),
            kind: BuffKind::Magic,
            priority: 0,
            turns_remaining,
            source_id: "gm".to_owned(),
            beneficial: true,
            effects: vec![BuffEffect {
                field: BuffField::DamageTakenModifier,
                value: BuffValue::Set(0.5),
            }],
            tick_actions: Vec::new(),
        }
    }

    #[test]
    fn approval_onboarding_text_uses_current_group_guide_for_private_player() {
        let mut manager = empty_manager();
        manager.messages.insert("2".to_owned(), vec![
            test_private_message(2),
        ]);
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            guide: "请先完成角色设定。".to_owned(),
            ..Default::default()
        });
        manager.current_trpg_group = Some("table".to_owned());

        assert!(manager.approve_chat_target("2"));

        assert_eq!(
            approval_onboarding_text(&manager, "2"),
            Some("团内引导：\n请先完成角色设定。".to_owned())
        );
    }

    #[test]
    fn approval_onboarding_text_supports_history_free_imported_private_target() {
        let mut manager = empty_manager();
        manager
            .chat_targets
            .insert("2".to_owned(), Default::default());
        manager.chat_target_kinds.insert(
            "2".to_owned(),
            ChatTargetExportKind::Private,
        );
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            guide: "导入后引导".to_owned(),
            ..Default::default()
        });
        manager.current_trpg_group = Some("table".to_owned());

        assert!(manager.approve_chat_target("2"));
        assert_eq!(
            approval_onboarding_text(&manager, "2"),
            Some("团内引导：\n导入后引导".to_owned())
        );
    }

    #[test]
    fn approval_onboarding_text_uses_known_noncurrent_group_guide() {
        let mut manager = empty_manager();
        manager.messages.insert("2".to_owned(), vec![
            test_private_message(2),
        ]);
        manager.trpg_groups.insert("alpha".to_owned(), TrpgGroup {
            guide: "alpha secret".to_owned(),
            players: vec!["9".to_owned()],
            ..Default::default()
        });
        manager.trpg_groups.insert("beta".to_owned(), TrpgGroup {
            guide: "beta guide".to_owned(),
            players: vec!["2".to_owned()],
            ..Default::default()
        });
        manager.current_trpg_group = Some("alpha".to_owned());

        assert!(manager.approve_chat_target("2"));

        assert_eq!(
            approval_onboarding_text(&manager, "2"),
            Some("团内引导：\nbeta guide".to_owned())
        );
        assert!(!manager.trpg_groups["alpha"]
            .players
            .contains(&"2".to_owned()));
    }

    #[test]
    fn approval_onboarding_text_skips_group_targets_and_empty_guides() {
        let mut manager = empty_manager();
        manager.messages.insert("2".to_owned(), vec![
            test_private_message(2),
        ]);
        manager.messages.insert("99".to_owned(), vec![
            test_group_message(4, "hello"),
        ]);
        manager
            .trpg_groups
            .insert("table".to_owned(), TrpgGroup::default());
        manager.current_trpg_group = Some("table".to_owned());

        assert!(manager.approve_chat_target("2"));
        assert!(manager.approve_chat_target("99"));

        assert_eq!(
            approval_onboarding_text(&manager, "2"),
            None
        );
        assert_eq!(
            approval_onboarding_text(&manager, "99"),
            None
        );
    }

    #[test]
    fn group_broadcast_targets_default_to_all_private_members_deduped() {
        let mut manager = empty_manager();
        for user_id in [2, 3, 4] {
            manager.messages.insert(user_id.to_string(), vec![
                test_private_message(user_id),
            ]);
        }
        let members = vec![
            "2".to_owned(),
            "3".to_owned(),
            "2".to_owned(),
            "4".to_owned(),
            "missing".to_owned(),
        ];

        let targets = group_broadcast_targets(
            None,
            &members,
            &manager,
            BROADCAST_SCOPE_ALL,
        );

        assert_eq!(targets, vec![
            NapcatSendTarget::Private(2),
            NapcatSendTarget::Private(3),
            NapcatSendTarget::Private(4),
        ]);
    }

    #[test]
    fn group_broadcast_targets_filter_to_selected_party() {
        let mut manager = empty_manager();
        for user_id in [2, 3, 4] {
            manager.messages.insert(user_id.to_string(), vec![
                test_private_message(user_id),
            ]);
        }
        let mut group = TrpgGroup {
            players: vec!["2".to_owned(), "3".to_owned(), "4".to_owned()],
            ..Default::default()
        };
        group.ensure_party("red");
        group.ensure_party("blue");
        group.set_player_party("2", Some("red"));
        group.set_player_party("3", Some("red"));
        group.set_player_party("4", Some("blue"));
        let members = vec!["2".to_owned(), "3".to_owned(), "4".to_owned()];

        let targets = group_broadcast_targets(
            Some(&group),
            &members,
            &manager,
            &broadcast_party_scope("red"),
        );

        assert_eq!(targets, vec![
            NapcatSendTarget::Private(2),
            NapcatSendTarget::Private(3),
        ]);
    }

    #[test]
    fn group_broadcast_targets_party_scope_requires_current_group() {
        let mut manager = empty_manager();
        manager.messages.insert("2".to_owned(), vec![
            test_private_message(2),
        ]);
        let members = vec!["2".to_owned()];

        let targets = group_broadcast_targets(
            None,
            &members,
            &manager,
            &broadcast_party_scope("red"),
        );

        assert!(targets.is_empty());
    }

    #[test]
    fn group_broadcast_targets_expand_imported_legacy_send_pane() {
        let mut manager = empty_manager();
        for user_id in [10002, 10003, 10004, 10005] {
            manager.messages.insert(user_id.to_string(), vec![
                test_private_message(user_id),
            ]);
        }
        let group = TrpgGroup {
            players: vec![
                "10002".to_owned(),
                "10003".to_owned(),
                "10004".to_owned(),
                "10005".to_owned(),
            ],
            legacy_teams: vec![crate::napcat::TrpgLegacyTeam {
                id: "1".to_owned(),
                name: "红队频道".to_owned(),
                players: vec!["10002".to_owned(), "10003".to_owned()],
                ..Default::default()
            }],
            legacy_worlds: vec![crate::napcat::TrpgLegacyWorld {
                id: "world-a".to_owned(),
                name: "旧世界".to_owned(),
                chat_areas: vec![crate::napcat::TrpgLegacyArea {
                    id: "area-a".to_owned(),
                    name: "密谈区".to_owned(),
                    members: vec!["10003".to_owned(), "10004".to_owned()],
                    ..Default::default()
                }],
                ..Default::default()
            }],
            legacy_send_panes: vec![crate::napcat::TrpgLegacySendPane {
                key: "7".to_owned(),
                title: "混合发送窗".to_owned(),
                targets: vec![
                    "1".to_owned(),
                    "area-a".to_owned(),
                    "10005".to_owned(),
                    "10003".to_owned(),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let members = vec!["10002".to_owned()];

        let targets = group_broadcast_targets(
            Some(&group),
            &members,
            &manager,
            &broadcast_legacy_pane_scope("7"),
        );

        assert_eq!(
            group.legacy_send_pane_effective_targets("7"),
            vec!["1".to_owned(), "area-a".to_owned(), "10005".to_owned(),]
        );
        assert_eq!(
            group.legacy_send_pane_disabled_direct_targets("7"),
            vec!["10003".to_owned()]
        );
        assert_eq!(targets, vec![
            NapcatSendTarget::Private(10002),
            NapcatSendTarget::Private(10003),
            NapcatSendTarget::Private(10004),
            NapcatSendTarget::Private(10005),
        ]);
    }

    #[test]
    fn legacy_send_pane_all_target_disables_other_targets() {
        let mut manager = empty_manager();
        for user_id in [10002, 10003] {
            manager.messages.insert(user_id.to_string(), vec![
                test_private_message(user_id),
            ]);
        }
        let group = TrpgGroup {
            players: vec!["10002".to_owned(), "10003".to_owned()],
            legacy_send_panes: vec![crate::napcat::TrpgLegacySendPane {
                key: "all".to_owned(),
                title: "全员发送窗".to_owned(),
                targets: vec!["0".to_owned(), "10002".to_owned(), "10003".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };

        assert_eq!(
            group.legacy_send_pane_effective_targets("all"),
            vec!["0".to_owned()]
        );
        assert_eq!(
            legacy_send_pane_targets(&manager, &group, "all"),
            vec![
                NapcatSendTarget::Private(10002),
                NapcatSendTarget::Private(10003),
            ]
        );
    }

    #[test]
    fn legacy_send_pane_composer_queues_private_targets_until_ack() {
        let mut manager = empty_manager();
        for user_id in [10002, 10003] {
            manager.messages.insert(user_id.to_string(), vec![
                test_private_message(user_id),
            ]);
        }
        let group = TrpgGroup {
            players: vec!["10002".to_owned(), "10003".to_owned()],
            legacy_send_panes: vec![crate::napcat::TrpgLegacySendPane {
                key: "7".to_owned(),
                title: "密谈发送窗".to_owned(),
                targets: vec!["10003".to_owned(), "10002".to_owned(), "10002".to_owned()],
                ..Default::default()
            }],
            ..Default::default()
        };
        let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
        let sender = NapcatIOSender(sender);
        let mut ime = ImeManager::default();
        let input_id = legacy_send_pane_input_id("table", "7");

        let target_count = queue_legacy_send_pane_text(
            &manager,
            &group,
            "table",
            "7",
            " 秘密提示 ",
            &sender,
            &mut ime,
        )
        .unwrap();

        assert_eq!(target_count, 2);
        let first = receiver.try_recv().unwrap();
        let second = receiver.try_recv().unwrap();
        assert_eq!(first.target_id, input_id);
        assert_eq!(second.target_id, input_id);
        assert!(first.message.to_string().contains("秘密提示"));
        assert!(second.message.to_string().contains("秘密提示"));

        let sent = ime.apply_send_results([
            crate::napcat::NapcatSendResult {
                request_id: first.request_id,
                target_id: input_id.clone(),
                error: None,
            },
            crate::napcat::NapcatSendResult {
                request_id: second.request_id,
                target_id: input_id.clone(),
                error: None,
            },
        ]);

        assert_eq!(sent, vec![ChatInputSendCompletion {
            input_id,
            text: "秘密提示".to_owned(),
            successful_targets: vec![
                NapcatSendTarget::Private(10002),
                NapcatSendTarget::Private(10003),
            ],
            clear_input: true,
        }]);
    }

    #[test]
    fn legacy_team_chat_composer_queues_private_targets_until_ack() {
        let mut manager = empty_manager();
        for user_id in [10002, 10003] {
            manager.messages.insert(user_id.to_string(), vec![
                test_private_message(user_id),
            ]);
        }
        let group = TrpgGroup {
            players: vec!["10002".to_owned(), "10003".to_owned(), "10004".to_owned()],
            legacy_teams: vec![crate::napcat::TrpgLegacyTeam {
                id: "1".to_owned(),
                name: "红队频道".to_owned(),
                players: vec![
                    "10003".to_owned(),
                    "10002".to_owned(),
                    "10004".to_owned(),
                    "missing".to_owned(),
                ],
                ..Default::default()
            }],
            ..Default::default()
        };
        let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
        let sender = NapcatIOSender(sender);
        let mut ime = ImeManager::default();
        let input_id = legacy_team_chat_input_id("table", "1");

        assert_eq!(
            legacy_team_chat_targets(&manager, &group, "1"),
            vec![
                NapcatSendTarget::Private(10003),
                NapcatSendTarget::Private(10002),
            ]
        );
        let target_count = queue_legacy_team_chat_text(
            &manager,
            &group,
            "table",
            "1",
            " 红队提示 ",
            &sender,
            &mut ime,
        )
        .unwrap();

        assert_eq!(target_count, 2);
        let first = receiver.try_recv().unwrap();
        let second = receiver.try_recv().unwrap();
        assert_eq!(first.target_id, input_id);
        assert_eq!(second.target_id, input_id);
        assert!(first.message.to_string().contains("红队提示"));
        assert!(second.message.to_string().contains("红队提示"));

        let sent = ime.apply_send_results([
            crate::napcat::NapcatSendResult {
                request_id: first.request_id,
                target_id: input_id.clone(),
                error: None,
            },
            crate::napcat::NapcatSendResult {
                request_id: second.request_id,
                target_id: input_id.clone(),
                error: None,
            },
        ]);

        assert_eq!(sent, vec![ChatInputSendCompletion {
            input_id,
            text: "红队提示".to_owned(),
            successful_targets: vec![
                NapcatSendTarget::Private(10003),
                NapcatSendTarget::Private(10002),
            ],
            clear_input: true,
        }]);
    }

    #[test]
    fn legacy_team_chat_window_entries_include_opened_old_channel_geometry() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            legacy_teams: vec![
                crate::napcat::TrpgLegacyTeam {
                    id: "2".to_owned(),
                    window_x: 12.0,
                    window_y: 34.0,
                    window_width: 240.0,
                    window_height: 120.0,
                    visible: false,
                    ..Default::default()
                },
                crate::napcat::TrpgLegacyTeam {
                    id: "1".to_owned(),
                    name: "红队频道".to_owned(),
                    ..Default::default()
                },
            ],
            ..Default::default()
        });

        let mut open_windows = HashSet::from([
            ("table".to_owned(), "2".to_owned()),
            ("table".to_owned(), "missing".to_owned()),
        ]);
        let entries = legacy_team_chat_window_entries(&manager, &open_windows);
        assert_eq!(entries.len(), 1);
        assert_eq!(entries[0].team_id, "2");
        assert_eq!(entries[0].title, "旧频道 2");
        assert!(!entries[0].visible);
        assert_eq!(
            entries[0].default_pos,
            Some(Pos2::new(12.0, 34.0))
        );
        assert_eq!(
            entries[0].default_size,
            Vec2::new(320.0, 260.0)
        );

        open_windows.insert(("table".to_owned(), "1".to_owned()));
        let entries = legacy_team_chat_window_entries(&manager, &open_windows);
        let mut team_ids = entries
            .iter()
            .map(|entry| entry.team_id.as_str())
            .collect::<Vec<_>>();
        team_ids.sort();
        assert_eq!(team_ids, vec!["1", "2"]);
    }

    #[test]
    fn legacy_send_pane_window_entries_include_fixed_and_open_panes() {
        let mut manager = empty_manager();
        manager.trpg_groups.insert("table".to_owned(), TrpgGroup {
            legacy_send_panes: vec![
                crate::napcat::TrpgLegacySendPane {
                    key: "fixed".to_owned(),
                    title: "固定发送窗".to_owned(),
                    targets: vec!["10002".to_owned()],
                    closable: false,
                },
                crate::napcat::TrpgLegacySendPane {
                    key: "closable".to_owned(),
                    title: "可关发送窗".to_owned(),
                    targets: vec!["10003".to_owned()],
                    closable: true,
                },
            ],
            ..Default::default()
        });

        let mut open_windows = HashSet::from([("table".to_owned(), "missing".to_owned())]);
        let fixed_only = legacy_send_pane_window_entries(&manager, &open_windows);
        assert_eq!(
            fixed_only
                .iter()
                .map(|entry| entry.pane_key.as_str())
                .collect::<Vec<_>>(),
            vec!["fixed"]
        );

        open_windows.insert((
            "table".to_owned(),
            "closable".to_owned(),
        ));
        let entries = legacy_send_pane_window_entries(&manager, &open_windows);
        let mut pane_keys = entries
            .iter()
            .map(|entry| entry.pane_key.as_str())
            .collect::<Vec<_>>();
        pane_keys.sort();
        assert_eq!(pane_keys, vec!["closable", "fixed"]);
    }

    #[test]
    fn no_active_campaign_disables_player_unread_and_summary_surfaces() {
        let manager = empty_manager();
        let messages = vec![test_private_message(2)];
        let mut deepseek_manager = DeepseekManager::default();

        assert!(summary_scopes_for_target(&manager, "2", &messages).is_empty());
        assert!(campaign_messages_for_summary_scope(
            &manager,
            "2",
            &messages,
            &SummaryScope::Private,
        )
        .is_empty());
        assert_eq!(
            target_unread_count_for_player(&manager, "2", &messages, 2),
            0
        );
        assert!(!queue_summaries_if_needed(
            &manager,
            "2",
            &messages,
            &HashMap::default(),
            None,
            &mut deepseek_manager,
        ));
        assert!(deepseek_manager.summaries.is_empty());
    }

    #[test]
    fn group_summary_scope_filters_public_and_party_messages() {
        let manager = split_party_summary_manager();
        let mut other_campaign = test_group_message(2, "other campaign secret");
        other_campaign.data.campaign_id = "other".to_owned();
        let messages = vec![
            test_group_message(4, "public clue"),
            test_group_message(2, "red clue"),
            test_group_message(3, "blue clue"),
            other_campaign,
        ];

        let public_lines = player_text_lines(&campaign_messages_for_summary_scope(
            &manager,
            "99",
            &messages,
            &SummaryScope::GroupPublic,
        ));
        let red_lines = player_text_lines(&campaign_messages_for_summary_scope(
            &manager,
            "99",
            &messages,
            &SummaryScope::GroupParty("red".to_owned()),
        ));

        let public_text = public_lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");
        let red_text = red_lines
            .iter()
            .map(|line| line.text.as_str())
            .collect::<Vec<_>>()
            .join("\n");

        assert!(public_text.contains("public clue"));
        assert!(!public_text.contains("red clue"));
        assert!(!public_text.contains("blue clue"));
        assert!(red_text.contains("public clue"));
        assert!(red_text.contains("red clue"));
        assert!(!red_text.contains("blue clue"));
        assert!(!public_text.contains("other campaign secret"));
        assert!(!red_text.contains("other campaign secret"));
    }

    #[test]
    fn group_chat_summary_requests_use_scoped_keys_and_filtered_payloads() {
        let manager = split_party_summary_manager();
        let mut messages = Vec::new();
        for index in 0..5 {
            messages.push(test_group_message(
                4,
                &format!("public clue {index}"),
            ));
            messages.push(test_group_message(
                2,
                &format!("red clue {index}"),
            ));
            messages.push(test_group_message(
                3,
                &format!("blue clue {index}"),
            ));
        }
        let (sender, mut receiver) = tokio::sync::mpsc::channel(8);
        let deepseek_sender = DeepseekIOSender(sender);
        let mut deepseek_manager = DeepseekManager::default();

        assert!(queue_summaries_if_needed(
            &manager,
            "99",
            &messages,
            &HashMap::default(),
            Some(&deepseek_sender),
            &mut deepseek_manager,
        ));

        let public_key = SummaryScope::GroupPublic.summary_key("default", "99");
        let red_key = SummaryScope::GroupParty("red".to_owned()).summary_key("default", "99");
        let blue_key = SummaryScope::GroupParty("blue".to_owned()).summary_key("default", "99");
        assert!(deepseek_manager.summaries.contains_key(&public_key));
        assert!(deepseek_manager.summaries.contains_key(&red_key));
        assert!(deepseek_manager.summaries.contains_key(&blue_key));
        assert!(!deepseek_manager.summaries.contains_key("99"));
        assert_ne!(
            red_key,
            SummaryScope::GroupParty("red".to_owned()).summary_key("other", "99")
        );

        let mut request_texts = HashMap::new();
        while let Ok(message) = receiver.try_recv() {
            let Message::Text(text) = message else {
                continue;
            };
            let DeepseekRequest::Summary {
                target_id, text, ..
            } = serde_json::from_str::<DeepseekRequest>(&text)
                .expect("summary request should deserialize")
            else {
                continue;
            };
            request_texts.insert(target_id, text);
        }

        let public_text = &request_texts[&public_key];
        let red_text = &request_texts[&red_key];
        let blue_text = &request_texts[&blue_key];
        assert!(public_text.contains("public clue 0"));
        assert!(!public_text.contains("red clue 0"));
        assert!(!public_text.contains("blue clue 0"));
        assert!(red_text.contains("public clue 0"));
        assert!(red_text.contains("red clue 0"));
        assert!(!red_text.contains("blue clue 0"));
        assert!(blue_text.contains("public clue 0"));
        assert!(!blue_text.contains("red clue 0"));
        assert!(blue_text.contains("blue clue 0"));
    }

    #[test]
    fn inventory_stacks_matching_stackable_items() {
        let mut inventory = CharacterInventory::default();
        add_item_to_inventory(&mut inventory, InventoryItem {
            name: "治疗药水".to_owned(),
            stack: 3,
            max_stack: 5,
            ..Default::default()
        });
        add_item_to_inventory(&mut inventory, InventoryItem {
            name: "治疗药水".to_owned(),
            stack: 4,
            max_stack: 5,
            ..Default::default()
        });

        assert_eq!(inventory.items.len(), 2);
        assert_eq!(inventory.items[0].stack, 5);
        assert_eq!(inventory.items[1].stack, 2);
    }

    #[test]
    fn equipped_item_effects_apply_and_are_removed_from_final_stats() {
        let mut character = PlayerCharacter::default();
        character
            .inventory
            .equipment
            .insert(EquipmentSlot::Feet, InventoryItem {
                name: "疾风靴".to_owned(),
                equipment_slot: EquipmentSlot::Feet,
                stat_effects: vec![
                    BuffEffect {
                        field: BuffField::Speed,
                        value: BuffValue::Add(2.0),
                    },
                    BuffEffect {
                        field: BuffField::MaxHp,
                        value: BuffValue::Add(10.0),
                    },
                ],
                ..Default::default()
            });
        let mut rules = RuleEngineState::default();
        let config = TrpgBasicConfig::default();

        sync_character_buffs(
            "player",
            &mut character,
            &config,
            &mut rules,
            &[],
        );

        assert!((character.speed - 5.0).abs() < 0.0001);
        assert!((character.max_hp - 15.0).abs() < 0.0001);
        assert!(character.buff_base_stats.is_some());

        character.inventory.equipment.clear();
        sync_character_buffs(
            "player",
            &mut character,
            &config,
            &mut rules,
            &[],
        );

        assert!((character.speed - 3.0).abs() < 0.0001);
        assert!((character.max_hp - 5.0).abs() < 0.0001);
        assert!(character.buff_base_stats.is_none());
    }

    #[test]
    fn equipping_item_moves_previous_item_to_bag() {
        let mut inventory = CharacterInventory::default();
        inventory.items.push(InventoryItem {
            name: "旧剑".to_owned(),
            equipment_slot: EquipmentSlot::MainHand,
            ..Default::default()
        });
        inventory.items.push(InventoryItem {
            name: "新剑".to_owned(),
            equipment_slot: EquipmentSlot::MainHand,
            ..Default::default()
        });

        equip_inventory_item(&mut inventory, 0);
        equip_inventory_item(&mut inventory, 0);

        assert_eq!(
            inventory.equipment[&EquipmentSlot::MainHand].name,
            "新剑"
        );
        assert_eq!(inventory.items.len(), 1);
        assert_eq!(inventory.items[0].name, "旧剑");
    }

    #[test]
    fn removing_inventory_and_skill_entries_repairs_hotbar_indexes() {
        let mut character = PlayerCharacter::default();
        character.inventory.items = vec![
            InventoryItem {
                name: "药水".to_owned(),
                ..Default::default()
            },
            InventoryItem {
                name: "绳索".to_owned(),
                ..Default::default()
            },
        ];
        character.skill_names = vec!["被移除".to_owned(), "冲刺".to_owned()];
        character.inventory.hotbar = vec![
            CharacterHotbarSlot::Item(0),
            CharacterHotbarSlot::Item(1),
            CharacterHotbarSlot::Skill(0),
            CharacterHotbarSlot::Skill(1),
        ];

        remove_character_inventory_item(&mut character, 0, false);
        shift_character_hotbar_after_remove(
            &mut character.inventory.hotbar,
            CharacterHotbarSlot::Skill(0),
        );

        assert_eq!(character.inventory.hotbar, vec![
            CharacterHotbarSlot::Empty,
            CharacterHotbarSlot::Item(0),
            CharacterHotbarSlot::Empty,
            CharacterHotbarSlot::Skill(0),
        ]);
    }

    #[test]
    fn random_pool_ignores_disabled_and_zero_weight_entries() {
        let pool = RandomPool {
            entries: vec![
                RandomPoolEntry {
                    item: InventoryItem {
                        name: "不会出现".to_owned(),
                        ..Default::default()
                    },
                    weight: 999.0,
                    enabled: false,
                    ..Default::default()
                },
                RandomPoolEntry {
                    item: InventoryItem {
                        name: "也不会出现".to_owned(),
                        ..Default::default()
                    },
                    weight: 0.0,
                    enabled: true,
                    ..Default::default()
                },
                RandomPoolEntry {
                    item: InventoryItem {
                        name: "固定结果".to_owned(),
                        ..Default::default()
                    },
                    weight: 1.0,
                    enabled: true,
                    ..Default::default()
                },
            ],
            last_pick: None,
            last_text_result: None,
            ..Default::default()
        };

        assert_eq!(random_pool_total_weight(&pool), 1.0);
        assert_eq!(
            pick_random_pool_item(&pool).unwrap().name,
            "固定结果"
        );
    }

    #[test]
    fn random_pool_text_result_uses_fixed_count_and_label() {
        let entry = RandomPoolEntry {
            item: InventoryItem {
                name: "事件".to_owned(),
                ..Default::default()
            },
            result_text: "获得线索".to_owned(),
            min_count: 2,
            max_count: 2,
            ..Default::default()
        };

        let result = random_pool_entry_text_result(&entry).unwrap();

        assert_eq!(result.entry_name, "事件");
        assert_eq!(result.text, "获得线索");
        assert_eq!(result.count, 2);
        assert_eq!(
            random_pool_text_result_label(&result),
            "获得线索 x2"
        );
    }

    #[test]
    fn random_pool_batch_message_formats_numbered_results() {
        let pool = RandomPool {
            entries: vec![RandomPoolEntry {
                item: InventoryItem {
                    name: "事件".to_owned(),
                    ..Default::default()
                },
                result_text: "获得线索".to_owned(),
                min_count: 2,
                max_count: 2,
                weight: 1.0,
                enabled: true,
                ..Default::default()
            }],
            last_pick: None,
            last_text_result: None,
            ..Default::default()
        };

        let results = random_pool_batch_text_results(&pool, 2);
        let message = random_pool_text_results_message("遭遇随机", &results).unwrap();

        assert_eq!(results.len(), 2);
        assert_eq!(
            message,
            "遭遇随机随机结果\n1. 获得线索 x2\n2. 获得线索 x2"
        );
    }

    #[test]
    fn random_pool_batch_message_omits_blank_text_results() {
        let pool = RandomPool {
            entries: vec![RandomPoolEntry {
                item: InventoryItem {
                    name: "只有物品".to_owned(),
                    ..Default::default()
                },
                result_text: "  ".to_owned(),
                weight: 1.0,
                enabled: true,
                ..Default::default()
            }],
            last_pick: None,
            last_text_result: None,
            ..Default::default()
        };

        let results = random_pool_batch_text_results(&pool, 3);

        assert!(results.is_empty());
        assert!(random_pool_text_results_message("空池", &results).is_none());
    }

    #[test]
    fn random_pool_legacy_group_and_tag_filters_match_metadata() {
        let mut manager = empty_manager();
        manager
            .random_pools
            .insert("探索池".to_owned(), RandomPool {
                legacy_pool_id: Some("random-a".to_owned()),
                legacy_group: Some(2),
                tags: "探索 战斗 探索".to_owned(),
                description: "旧描述".to_owned(),
                created_at: "2024-01-02".to_owned(),
                ..Default::default()
            });
        manager
            .random_pools
            .insert("无标签池".to_owned(), RandomPool {
                legacy_group: None,
                ..Default::default()
            });
        let pool = &manager.random_pools["探索池"];

        assert_eq!(
            random_pool_tag_tokens(&pool.tags),
            vec!["战斗".to_owned(), "探索".to_owned()]
        );
        assert_eq!(
            random_pool_metadata_summary(pool),
            "旧团索引 2 · 标签 战斗 探索 · 创建 2024-01-02"
        );
        assert!(random_pool_matches_filters(
            pool,
            &random_pool_group_filter_value(Some(2)),
            "探索",
        ));
        assert!(!random_pool_matches_filters(
            pool,
            &random_pool_group_filter_value(Some(3)),
            "探索",
        ));
        assert!(!random_pool_matches_filters(
            pool,
            RANDOM_POOL_FILTER_ALL,
            RANDOM_POOL_FILTER_UNTAGGED,
        ));
        assert!(random_pool_matches_filters(
            &manager.random_pools["无标签池"],
            RANDOM_POOL_FILTER_UNGROUPED,
            RANDOM_POOL_FILTER_UNTAGGED,
        ));

        let (groups, tags) = random_pool_filter_options(&manager);
        assert_eq!(groups, vec![None, Some(2)]);
        assert_eq!(tags, vec![
            "战斗".to_owned(),
            "探索".to_owned()
        ]);
    }

    #[test]
    fn random_pool_checked_results_assign_unique_private_targets() {
        let pool = RandomPool {
            entries: vec![
                RandomPoolEntry {
                    result_text: "遭遇伏击".to_owned(),
                    min_count: 2,
                    max_count: 2,
                    enabled: true,
                    ..Default::default()
                },
                RandomPoolEntry {
                    result_text: "人数不足时跳过".to_owned(),
                    min_count: 2,
                    max_count: 2,
                    enabled: true,
                    ..Default::default()
                },
            ],
            ..Default::default()
        };
        let targets = vec![
            NapcatSendTarget::Private(10002),
            NapcatSendTarget::Private(10003),
            NapcatSendTarget::Group(20001),
        ];

        let results = random_pool_checked_results(&pool, &targets);

        assert_eq!(results.len(), 2);
        assert!(results.iter().all(|result| result.enabled));
        assert!(results.iter().all(|result| result.text == "遭遇伏击"));
        let target_ids = results
            .iter()
            .map(|result| result.target_id.as_str())
            .collect::<HashSet<_>>();
        assert_eq!(target_ids.len(), 2);
        assert!(target_ids.contains("10002"));
        assert!(target_ids.contains("10003"));
    }

    #[test]
    fn random_pool_checked_results_queue_enabled_private_rows() {
        let pool = RandomPool {
            checked_results: vec![
                RandomPoolCheckedResult {
                    enabled: true,
                    target_id: "10002".to_owned(),
                    text: "  你发现了线索  ".to_owned(),
                },
                RandomPoolCheckedResult {
                    enabled: false,
                    target_id: "10003".to_owned(),
                    text: "不会发送".to_owned(),
                },
                RandomPoolCheckedResult {
                    enabled: true,
                    target_id: "10004".to_owned(),
                    text: "   ".to_owned(),
                },
            ],
            ..Default::default()
        };
        let (sender, mut receiver) = tokio::sync::mpsc::channel(4);
        let sender = NapcatIOSender(sender);
        let mut ime = ImeManager::default();

        let sent_count =
            queue_random_pool_checked_results("遭遇随机", &pool, &sender, &mut ime).unwrap();

        assert_eq!(sent_count, 1);
        let outbound = receiver.try_recv().unwrap();
        assert_eq!(
            outbound.target_id,
            random_pool_checked_send_input_id("遭遇随机", 0)
        );
        assert!(outbound.message.to_string().contains("send_private_msg"));
        assert!(outbound.message.to_string().contains("10002"));
        assert!(outbound.message.to_string().contains("你发现了线索"));
        assert!(receiver.try_recv().is_err());

        let sent = ime.apply_send_results([crate::napcat::NapcatSendResult {
            request_id: outbound.request_id,
            target_id: outbound.target_id,
            error: None,
        }]);

        assert_eq!(sent, vec![ChatInputSendCompletion {
            input_id: random_pool_checked_send_input_id("遭遇随机", 0),
            text: "你发现了线索".to_owned(),
            successful_targets: vec![NapcatSendTarget::Private(10002)],
            clear_input: true,
        }]);
    }

    #[test]
    fn skill_pool_entry_copies_to_character_skills() {
        let mut character = PlayerCharacter::default();
        let entry = SkillPoolEntry {
            name: "烈焰箭".to_owned(),
            note: "主动使用对目标造成3点魔法伤害".to_owned(),
            mp_cost: 2.0,
            cooldown_turns: 1,
            source_character_id: Some("player-1".to_owned()),
            source_character_name: Some("法师".to_owned()),
            source_skill_index: Some(0),
            legacy_pool_id: Some("legacy-fire".to_owned()),
            category: Some("普通".to_owned()),
            legacy_has_graph: true,
            legacy_graph_json: Some(r#"{"nodes":[{"id":"n1"}]}"#.to_owned()),
            ..Default::default()
        };

        add_skill_pool_entry_to_character(&mut character, &entry);

        assert_eq!(character.skill_names, vec![
            "烈焰箭".to_owned()
        ]);
        assert_eq!(character.skill_notes, vec![
            "主动使用对目标造成3点魔法伤害".to_owned()
        ]);
        assert_eq!(character.skill_mp_costs, vec![2.0]);
        assert_eq!(character.skill_cooldown_turns, vec![1]);
        assert_eq!(character.skill_metadata.len(), 1);
        assert_eq!(
            character.skill_metadata[0].source,
            CharacterSkillSourceKind::SkillPool
        );
        assert_eq!(
            character.skill_metadata[0].source_character_id.as_deref(),
            Some("player-1")
        );
        assert_eq!(
            character.skill_metadata[0].source_pool_id.as_deref(),
            Some("legacy-fire")
        );
        assert_eq!(
            character.skill_metadata[0].source_pool_label.as_deref(),
            Some("烈焰箭")
        );
        assert!(character.skill_metadata[0].legacy_has_buff_machine);
        assert_eq!(
            character.skill_metadata[0]
                .legacy_buff_machine_json
                .as_deref(),
            Some(r#"{"graph":{"nodes":[{"id":"n1"}]}}"#)
        );
    }

    #[test]
    fn advancing_character_buffs_decrements_and_expires_turn_limited_buffs() {
        let mut character = PlayerCharacter {
            active_buffs: vec![buff("expires", 1), buff("continues", 3)],
            ..Default::default()
        };

        assert!(advance_character_buffs(&mut character));

        assert_eq!(character.active_buffs.len(), 1);
        assert_eq!(
            character.active_buffs[0].name,
            "continues"
        );
        assert_eq!(
            character.active_buffs[0].turns_remaining,
            2
        );
    }

    #[test]
    fn advancing_character_buffs_preserves_zero_turn_permanent_buffs() {
        let mut character = PlayerCharacter {
            active_buffs: vec![buff("permanent", 0), buff("expires", 1)],
            ..Default::default()
        };

        assert!(advance_character_buffs(&mut character));

        assert_eq!(character.active_buffs.len(), 1);
        assert_eq!(
            character.active_buffs[0].name,
            "permanent"
        );
        assert_eq!(
            character.active_buffs[0].turns_remaining,
            0
        );
    }

    #[test]
    fn legacy_passive_buff_machine_applies_to_effective_character_stats() {
        let config = TrpgBasicConfig::default();
        let mut rule_engine_state = RuleEngineState::default();
        let mut character = PlayerCharacter {
            level: 1,
            status: CharacterStatus {
                str_: 1,
                ..Default::default()
            },
            skill_metadata: vec![CharacterSkillMetadata {
                args: vec![crate::napcat::SkillPoolArg {
                    name: "力量".to_owned(),
                    kind: "数字".to_owned(),
                    value: "3".to_owned(),
                }],
                legacy_has_buff_machine: true,
                legacy_buff_machine_json: Some(
                    r#"{"buffMachine":{"被动":[{"name":"强壮","prior":1,"effect":["str","DMGModify"],"type":1,"benifit":true,"value":["力量","25%"]}]}}"#
                        .to_owned(),
                ),
                ..Default::default()
            }],
            ..Default::default()
        };
        update_character_from_status_with_config(&mut character, &config);
        let base_max_hp = character.max_hp;

        sync_character_buffs(
            "caster",
            &mut character,
            &config,
            &mut rule_engine_state,
            &[],
        );

        assert!(character.active_buffs.is_empty());
        assert_eq!(character.extra_status.str_, 3);
        assert_eq!(
            character
                .buff_base_stats
                .as_ref()
                .unwrap()
                .extra_status
                .str_,
            0
        );
        assert!((character.damage_dealt_modifier - 1.25).abs() < 0.0001);
        assert!((character.max_hp - (base_max_hp + config.str_max_hp * 3.0)).abs() < 0.0001);
        assert_eq!(
            rule_engine_state.active_buff_names("caster"),
            vec!["强壮".to_owned()]
        );

        character.skill_metadata.clear();
        sync_character_buffs(
            "caster",
            &mut character,
            &config,
            &mut rule_engine_state,
            &[],
        );

        assert!(character.buff_base_stats.is_none());
        assert_eq!(character.extra_status.str_, 0);
        assert!((character.max_hp - base_max_hp).abs() < 0.0001);
        assert!((character.damage_dealt_modifier - 1.0).abs() < 0.0001);
    }

    #[test]
    fn moonberry_passive_talents_apply_to_effective_character_stats() {
        let config = TrpgBasicConfig::default();
        let mut rule_engine_state = RuleEngineState::default();
        let mut character = PlayerCharacter {
            level: 1,
            status: CharacterStatus {
                k: 3,
                int_: 5,
                wis: 4,
                ..Default::default()
            },
            skill_names: vec![
                "大魔法师".to_owned(),
                "矢量压缩能量池".to_owned(),
                "狡黠之思".to_owned(),
                "人类基因工程".to_owned(),
                "抗魔体质".to_owned(),
                "狂风恶浪".to_owned(),
                "忏悔".to_owned(),
            ],
            skill_metadata: vec![
                CharacterSkillMetadata::talent("normal_talent", "天赋"),
                CharacterSkillMetadata::talent("support_talent", "辅助天赋"),
                CharacterSkillMetadata::talent("support_talent", "辅助天赋"),
                CharacterSkillMetadata::talent("normal_talent", "天赋"),
                CharacterSkillMetadata::talent("support_talent", "辅助天赋"),
                CharacterSkillMetadata::talent("normal_talent", "天赋"),
                CharacterSkillMetadata::talent("support_talent", "辅助天赋"),
            ],
            ..Default::default()
        };
        update_character_from_status_with_config(&mut character, &config);
        let base_max_hp = character.max_hp;
        let base_max_mp = character.max_mp;
        let base_mp_regen = character.mp_regen;
        let base_speed = character.speed;

        sync_character_buffs(
            "caster",
            &mut character,
            &config,
            &mut rule_engine_state,
            &[],
        );

        assert!((character.max_hp - base_max_hp * 1.05).abs() < 0.0001);
        assert!((character.max_mp - (base_max_mp + 19.0)).abs() < 0.0001);
        assert!((character.mp_regen - (base_mp_regen + 4.0)).abs() < 0.0001);
        assert!((character.speed - base_speed * 1.2).abs() < 0.0001);
        let expected_healing_modifier =
            (1.0 + 5.0 * config.int_heal_bonus + 4.0 * config.wis_heal_bonus) * 1.03 * 1.25;
        assert!((character.healing_dealt_modifier - expected_healing_modifier).abs() < 0.0001);
        let synced = rule_engine_state.character("caster").unwrap();
        assert!((synced.magical_damage_taken_modifier - 0.9).abs() < 0.0001);
        assert!((synced.diseased_damage_taken_modifier - 0.85).abs() < 0.0001);
        assert!((synced.poisoning_damage_taken_modifier - 0.85).abs() < 0.0001);
        assert_eq!(
            character.buff_base_stats.as_ref().unwrap().max_mp,
            base_max_mp
        );
        let mut active_names = rule_engine_state.active_buff_names("caster");
        active_names.sort();
        assert_eq!(active_names, vec![
            "人类基因工程".to_owned(),
            "大魔法师".to_owned(),
            "忏悔".to_owned(),
            "狂风恶浪".to_owned(),
            "狡黠之思".to_owned(),
            "矢量压缩能量池".to_owned(),
        ]);

        character.skill_names.clear();
        character.skill_metadata.clear();
        sync_character_buffs(
            "caster",
            &mut character,
            &config,
            &mut rule_engine_state,
            &[],
        );

        assert!(character.buff_base_stats.is_none());
        assert!((character.max_hp - base_max_hp).abs() < 0.0001);
        assert!((character.max_mp - base_max_mp).abs() < 0.0001);
        assert!((character.mp_regen - base_mp_regen).abs() < 0.0001);
        assert!((character.speed - base_speed).abs() < 0.0001);
        assert!((character.healing_dealt_modifier - 1.0).abs() < 0.0001);
        let synced = rule_engine_state.character("caster").unwrap();
        assert!((synced.magical_damage_taken_modifier - 1.0).abs() < f32::EPSILON);
        assert!((synced.diseased_damage_taken_modifier - 1.0).abs() < f32::EPSILON);
        assert!((synced.poisoning_damage_taken_modifier - 1.0).abs() < f32::EPSILON);
        assert!(rule_engine_state.active_buff_names("caster").is_empty());
    }

    #[test]
    fn legacy_granted_buff_pool_syncs_to_rule_engine() {
        let config = TrpgBasicConfig::default();
        let mut rule_engine_state = RuleEngineState::default();
        let skill_pool = vec![SkillPoolEntry {
            name: "护盾池".to_owned(),
            legacy_pool_id: Some("shield-pool".to_owned()),
            args: vec![crate::napcat::SkillPoolArg {
                name: "护盾值".to_owned(),
                kind: "数字".to_owned(),
                value: "0.1".to_owned(),
            }],
            legacy_event_buff_json: Some(
                r#"[{"event":"技能释放","buffs":[{"name":"护盾","prior":2,"life":1,"effect":["tDMGModify"],"type":0,"from":"技能目标","benifit":true,"value":["护盾值"]}]}]"#
                    .to_owned(),
            ),
            ..Default::default()
        }];
        let caster = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            skill_names: vec!["给予护盾".to_owned()],
            skill_notes: vec![String::new()],
            skill_metadata: vec![CharacterSkillMetadata {
                args: vec![
                    crate::napcat::SkillPoolArg {
                        name: "护盾池".to_owned(),
                        kind: "BUFF".to_owned(),
                        value: "shield-pool".to_owned(),
                    },
                    crate::napcat::SkillPoolArg {
                        name: "减伤".to_owned(),
                        kind: "数字".to_owned(),
                        value: "0.25".to_owned(),
                    },
                ],
                legacy_has_buff_machine: true,
                legacy_buff_machine_json: Some(
                    r#"{"eventBuffs":[{"event":"技能释放","buffs":[{"name":"给予护盾","life":3,"effect":["给予BUFF"],"type":0,"from":"技能目标","benifit":true,"value":["护盾池","减伤"]}]}]}"#
                        .to_owned(),
                ),
                ..Default::default()
            }],
            ..Default::default()
        };
        let target = PlayerCharacter {
            hp: 10.0,
            max_hp: 10.0,
            ..Default::default()
        };

        sync_character_skill_rules(
            "caster",
            &caster,
            &config,
            &mut rule_engine_state,
            &skill_pool,
        );
        sync_character_skill_rules(
            "target",
            &target,
            &config,
            &mut rule_engine_state,
            &skill_pool,
        );
        rule_engine_state.cast_skill("caster", vec!["target".to_owned()]);

        assert_eq!(
            rule_engine_state.active_buff_names("target"),
            vec!["护盾".to_owned()]
        );
        let effective = rule_engine_state.character("target").unwrap();
        assert!((effective.damage_taken_modifier - 1.25).abs() < 0.0001);
    }

    #[test]
    fn quick_cast_legacy_granted_buff_pool_ticks_damage_on_turn_advance() {
        let mut manager = empty_manager();
        manager.skill_pool.push(SkillPoolEntry {
            name: "燃烧池".to_owned(),
            legacy_pool_id: Some("burn-pool".to_owned()),
            args: vec![crate::napcat::SkillPoolArg {
                name: "伤害值".to_owned(),
                kind: "数字".to_owned(),
                value: "1".to_owned(),
            }],
            legacy_event_buff_json: Some(
                r#"[{"event":"技能释放","buffs":[{"name":"燃烧伤害","effect":["伤害"],"from":"技能目标","value":["伤害值"]}]}]"#
                    .to_owned(),
            ),
            ..Default::default()
        });
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 10.0,
                max_mp: 10.0,
                skill_names: vec!["点燃".to_owned()],
                skill_notes: vec![String::new()],
                skill_metadata: vec![CharacterSkillMetadata {
                    args: vec![
                        crate::napcat::SkillPoolArg {
                            name: "燃烧池".to_owned(),
                            kind: "BUFF".to_owned(),
                            value: "burn-pool".to_owned(),
                        },
                        crate::napcat::SkillPoolArg {
                            name: "伤害值".to_owned(),
                            kind: "数字".to_owned(),
                            value: "3".to_owned(),
                        },
                    ],
                    legacy_has_buff_machine: true,
                    legacy_buff_machine_json: Some(
                        r#"{"eventBuffs":[{"event":"技能释放","buffs":[{"name":"燃烧","life":2,"effect":["给予BUFF"],"type":0,"from":"技能目标","benifit":false,"value":["燃烧池","伤害值"]}]}]}"#
                            .to_owned(),
                    ),
                    ..Default::default()
                }],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["caster".to_owned(), "target".to_owned()],
            ..Default::default()
        });
        manager.current_trpg_group = Some("party".to_owned());
        let skill = {
            let caster = manager.player_characters.get_mut("caster").unwrap();
            quick_cast_skills(caster).remove(0)
        };
        let effect = quick_cast_effect(
            &skill.note,
            &skill.arg_values,
            skill.skill_type.as_deref(),
            skill.legacy_buff_machine_json.as_deref(),
            &manager.skill_pool,
        );
        let Some(QuickCastEffect::GrantBuff { buff, .. }) = effect.as_ref() else {
            panic!("expected legacy granted buff quick-cast effect");
        };
        assert_eq!(buff.tick_actions, vec![
            BuffTickAction::Damage {
                amount: 3.0,
                damage_type: DamageType::Magical,
            }
        ]);

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect,
                cast_turn: 0,
                force: false,
            },
        ));
        let target = &manager.player_characters["target"];
        assert_eq!(target.active_buffs.len(), 1);
        assert_eq!(
            target.active_buffs[0].tick_actions,
            vec![BuffTickAction::Damage {
                amount: 3.0,
                damage_type: DamageType::Magical,
            }]
        );

        let mut rule_engine_state = RuleEngineState::default();
        assert!(advance_group_world_turn(
            &mut manager,
            "party",
            &mut rule_engine_state
        ));
        let target = &manager.player_characters["target"];
        assert_eq!(target.hp, 7.0);
        assert_eq!(target.damage_taken_this_turn, 3.0);
        assert_eq!(
            target.active_buffs[0].turns_remaining,
            1
        );
        assert_eq!(
            target.buff_base_stats.as_ref().unwrap().hp,
            7.0
        );

        assert!(advance_group_world_turn(
            &mut manager,
            "party",
            &mut rule_engine_state
        ));
        let target = &manager.player_characters["target"];
        assert_eq!(target.hp, 7.0);
        assert_eq!(target.damage_taken_this_turn, 0.0);
        assert!(target.active_buffs.is_empty());
        assert!(target.buff_base_stats.is_none());
    }

    #[test]
    fn healing_buff_tick_applies_mutual_aid_feedback_talent() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 20.0,
                skill_names: vec!["互帮互助".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata::talent("support_talent", "辅助天赋")],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 18.0,
                max_hp: 20.0,
                skill_names: vec!["互帮互助".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata::talent("support_talent", "辅助天赋")],
                ..Default::default()
            });

        assert!(apply_character_buff_ticks(
            &mut manager,
            &[CharacterBuffTick {
                source_id: "caster".to_owned(),
                target_id: "target".to_owned(),
                action: BuffTickAction::Heal { amount: 4.0 },
            }]
        ));

        let caster = &manager.player_characters["caster"];
        assert!((caster.hp - 12.0).abs() < 0.0001);
        assert!((caster.healing_taken_this_turn - 2.0).abs() < 0.0001);
        let target = &manager.player_characters["target"];
        assert!((target.hp - 20.0).abs() < 0.0001);
        assert!((target.healing_taken_this_turn - 2.0).abs() < 0.0001);
    }

    #[test]
    fn damage_buff_ticks_record_effective_overkill() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("source".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        manager.player_characters.insert(
            "normal-target".to_owned(),
            PlayerCharacter {
                hp: 2.0,
                max_hp: 20.0,
                ..Default::default()
            },
        );
        manager.player_characters.insert(
            "fixed-target".to_owned(),
            PlayerCharacter {
                hp: 1.0,
                max_hp: 20.0,
                ..Default::default()
            },
        );

        assert!(apply_character_buff_ticks(
            &mut manager,
            &[
                CharacterBuffTick {
                    source_id: "source".to_owned(),
                    target_id: "normal-target".to_owned(),
                    action: BuffTickAction::Damage {
                        amount: 4.0,
                        damage_type: DamageType::Physical,
                    },
                },
                CharacterBuffTick {
                    source_id: "source".to_owned(),
                    target_id: "fixed-target".to_owned(),
                    action: BuffTickAction::FixedDamage {
                        amount: 4.0,
                        damage_type: DamageType::None,
                    },
                },
            ]
        ));

        let target = &manager.player_characters["normal-target"];
        assert_eq!(target.hp, 0.0);
        assert_eq!(target.damage_taken_this_turn, 2.0);
        let target = &manager.player_characters["fixed-target"];
        assert_eq!(target.hp, 0.0);
        assert_eq!(target.damage_taken_this_turn, 1.0);
    }

    #[test]
    fn world_buff_ticks_apply_source_and_target_talent_modifiers() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("source".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                skill_names: vec!["生死时速".to_owned(), "混沌无序".to_owned()],
                skill_metadata: vec![
                    CharacterSkillMetadata::talent("support_talent", "辅助天赋"),
                    CharacterSkillMetadata::talent("normal_talent", "天赋"),
                ],
                ..Default::default()
            });
        manager.player_characters.insert(
            "damage-target".to_owned(),
            PlayerCharacter {
                hp: 20.0,
                max_hp: 20.0,
                skill_names: vec!["过度免疫".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata::talent("normal_talent", "天赋")],
                ..Default::default()
            },
        );
        manager.player_characters.insert(
            "healing-target".to_owned(),
            PlayerCharacter {
                hp: 4.0,
                max_hp: 20.0,
                ..Default::default()
            },
        );

        assert!(apply_character_buff_ticks(
            &mut manager,
            &[
                CharacterBuffTick {
                    source_id: "source".to_owned(),
                    target_id: "damage-target".to_owned(),
                    action: BuffTickAction::Damage {
                        amount: 5.0,
                        damage_type: DamageType::Physical,
                    },
                },
                CharacterBuffTick {
                    source_id: "source".to_owned(),
                    target_id: "healing-target".to_owned(),
                    action: BuffTickAction::Heal { amount: 4.0 },
                },
            ]
        ));

        let target = &manager.player_characters["damage-target"];
        assert!((3.4..=4.6).contains(&target.damage_taken_this_turn));
        assert!((15.4..=16.6).contains(&target.hp));
        let target = &manager.player_characters["healing-target"];
        assert!((5.1..=6.9).contains(&target.healing_taken_this_turn));
        assert!((9.1..=10.9).contains(&target.hp));
    }

    #[test]
    fn quick_cast_skills_exclude_unapproved_entries() {
        let mut character = PlayerCharacter {
            skill_names: vec!["已批准".to_owned(), "待批准".to_owned()],
            skill_notes: vec![
                "主动使用对目标造成1点物理伤害".to_owned(),
                "主动使用对目标造成9点物理伤害".to_owned(),
            ],
            skill_metadata: vec![CharacterSkillMetadata::default(), CharacterSkillMetadata {
                st_approved: false,
                ..Default::default()
            }],
            ..Default::default()
        };

        let skills = quick_cast_skills(&mut character);

        assert_eq!(pending_gm_skill_count(&character), 1);
        assert_eq!(skills.len(), 1);
        assert_eq!(skills[0].name, "已批准");
    }

    #[test]
    fn quick_cast_records_and_blocks_skill_cooldown_until_turn_passes() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 10.0,
                max_mp: 10.0,
                skill_names: vec!["旋风斩".to_owned()],
                skill_cooldown_turns: vec![2],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["caster".to_owned(), "target".to_owned()],
            player_turns: HashMap::from([(
                "caster".to_owned(),
                crate::napcat::TrpgPlayerTurnState::default(),
            )]),
            ..Default::default()
        });
        let skill = QuickCastSkill {
            index: 0,
            name: "旋风斩".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 2,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let effect = QuickCastEffect::Damage {
            amount: 1.0,
            target: TargetSelector {
                actor: ActorRef::Target,
                area: None,
            },
            damage_type: DamageType::Physical,
        };

        assert_eq!(
            quick_cast_cooldown_turn(&manager, "caster"),
            0
        );
        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill: skill.clone(),
                targets: vec!["target".to_owned()],
                effect: Some(effect.clone()),
                cast_turn: 0,
                force: false,
            },
        ));
        let caster = &manager.player_characters["caster"];
        assert_eq!(
            quick_skill_cooldown_remaining(caster, 0, 2, None, 0),
            2
        );

        assert!(!apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill: skill.clone(),
                targets: vec!["target".to_owned()],
                effect: Some(effect.clone()),
                cast_turn: 0,
                force: false,
            },
        ));

        manager
            .trpg_groups
            .get_mut("party")
            .unwrap()
            .set_player_turns_passed("caster", 2);
        assert_eq!(
            quick_cast_cooldown_turn(&manager, "caster"),
            2
        );
        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(effect),
                cast_turn: 2,
                force: false,
            },
        ));
    }

    #[test]
    fn quick_cast_blocks_imported_cooldown_left() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 10.0,
                max_mp: 10.0,
                skill_names: vec!["护盾".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata {
                    cooldown_left: Some(2),
                    ..Default::default()
                }],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        let skill = {
            let caster = manager.player_characters.get_mut("caster").unwrap();
            quick_cast_skills(caster).remove(0)
        };
        assert_eq!(skill.cooldown_left, Some(2));

        assert!(!apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill: skill.clone(),
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Damage {
                    amount: 1.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                    damage_type: DamageType::Physical,
                }),
                cast_turn: 0,
                force: false,
            },
        ));
        assert_eq!(
            manager.player_characters["target"].hp,
            10.0
        );
        assert_eq!(
            manager.player_characters["caster"].skill_cooldown_ready_turns["0"],
            2
        );

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Damage {
                    amount: 1.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                    damage_type: DamageType::Physical,
                }),
                cast_turn: 2,
                force: false,
            },
        ));
        assert_eq!(
            manager.player_characters["target"].hp,
            9.0
        );
        assert!(!manager.player_characters["caster"]
            .skill_cooldown_ready_turns
            .contains_key("0"));
    }

    #[test]
    fn quick_cast_executes_multi_action_effects_with_distinct_targets_and_single_cost() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 15.0,
                max_hp: 20.0,
                mp: 10.0,
                max_mp: 10.0,
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 20.0,
                ..Default::default()
            });
        let skill = QuickCastSkill {
            index: 0,
            name: "连段".to_owned(),
            note: "主动使用对目标造成3点物理伤害，对自己回复2点生命值".to_owned(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 3.0,
            cooldown_turns: 2,
            cooldown_left: None,
            target_count: Some(1),
            target_class: Some("单目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let mut effect = quick_cast_effect(
            &skill.note,
            &skill.arg_values,
            skill.skill_type.as_deref(),
            skill.legacy_buff_machine_json.as_deref(),
            &[],
        )
        .expect("multi-action skill should parse");
        let character_targets = vec![
            ("caster".to_owned(), "Caster".to_owned()),
            ("target".to_owned(), "Target".to_owned()),
        ];
        let resolved_targets = resolve_quick_cast_effect_targets(
            "caster",
            &manager.player_characters["caster"],
            &mut effect,
            &character_targets,
            None,
            None,
            &skill,
        );
        assert_eq!(resolved_targets, vec![
            "target".to_owned(),
            "caster".to_owned()
        ]);
        let QuickCastEffect::Sequence(effects) = effect else {
            panic!("multi-action skill should preserve its effect sequence");
        };
        assert_eq!(effects.len(), 2);
        assert_eq!(effects[0].targets, vec![
            "target".to_owned()
        ]);
        assert_eq!(effects[1].targets, vec![
            "caster".to_owned()
        ]);

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned(), "caster".to_owned()],
                effect: Some(QuickCastEffect::Sequence(effects)),
                cast_turn: 4,
                force: false,
            },
        ));

        let caster = &manager.player_characters["caster"];
        assert_eq!(caster.hp, 17.0);
        assert_eq!(caster.mp, 7.0);
        assert_eq!(
            caster.skill_last_cast_turns.get("0"),
            Some(&4)
        );
        assert!((manager.player_characters["target"].hp - 7.075).abs() < 0.0001);
    }

    #[test]
    fn quick_cast_limits_targets_by_metadata_target_count() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 10.0,
                max_mp: 10.0,
                ..Default::default()
            });
        manager
            .player_characters
            .insert("first".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        manager
            .player_characters
            .insert("second".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        let skill = QuickCastSkill {
            index: 0,
            name: "范围测试".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: Some(1),
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["first".to_owned(), "second".to_owned()],
                effect: Some(QuickCastEffect::Damage {
                    amount: 1.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                    damage_type: DamageType::Physical,
                }),
                cast_turn: 0,
                force: false,
            },
        ));
        assert_eq!(
            manager.player_characters["first"].hp,
            9.0
        );
        assert_eq!(
            manager.player_characters["second"].hp,
            10.0
        );
    }

    #[test]
    fn quick_cast_no_target_class_blocks_targets() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 10.0,
                max_mp: 10.0,
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        let skill = QuickCastSkill {
            index: 0,
            name: "无目标测试".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: Some(1),
            target_class: Some("无目标".to_owned()),
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Damage {
                    amount: 1.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                    damage_type: DamageType::Physical,
                }),
                cast_turn: 0,
                force: false,
            },
        ));
        assert_eq!(
            manager.player_characters["target"].hp,
            10.0
        );
    }

    #[test]
    fn quick_cast_uses_numeric_skill_args_in_amounts() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 10.0,
                max_mp: 10.0,
                skill_names: vec!["变量伤害".to_owned()],
                skill_notes: vec!["主动使用对目标造成伤害值点物理伤害".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata {
                    args: vec![crate::napcat::SkillPoolArg {
                        name: "伤害值".to_owned(),
                        kind: "数字".to_owned(),
                        value: "3".to_owned(),
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        let skill = {
            let caster = manager.player_characters.get_mut("caster").unwrap();
            quick_cast_skills(caster).remove(0)
        };
        let effect = quick_cast_effect(
            &skill.note,
            &skill.arg_values,
            skill.skill_type.as_deref(),
            None,
            &[],
        );

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect,
                cast_turn: 0,
                force: false,
            },
        ));
        assert_eq!(
            manager.player_characters["target"].hp,
            7.0
        );
    }

    #[test]
    fn quick_cast_uses_legacy_buff_machine_damage_when_note_unparsed() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 10.0,
                max_mp: 10.0,
                skill_names: vec!["旧蓝图伤害".to_owned()],
                skill_notes: vec!["旧月莓图形技能".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata {
                    skill_type: Some("远程".to_owned()),
                    args: vec![crate::napcat::SkillPoolArg {
                        name: "伤害值".to_owned(),
                        kind: "数字".to_owned(),
                        value: "4".to_owned(),
                    }],
                    legacy_has_buff_machine: true,
                    legacy_buff_machine_json: Some(
                        r#"{"技能释放":[{"name":"火球","effect":["伤害"],"type":7,"from":"技能目标","value":["伤害值"]}]}"#
                            .to_owned(),
                    ),
                    ..Default::default()
                }],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        let skill = {
            let caster = manager.player_characters.get_mut("caster").unwrap();
            quick_cast_skills(caster).remove(0)
        };
        let effect = quick_cast_effect(
            &skill.note,
            &skill.arg_values,
            skill.skill_type.as_deref(),
            skill.legacy_buff_machine_json.as_deref(),
            &[],
        );

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect,
                cast_turn: 0,
                force: false,
            },
        ));
        assert_eq!(
            manager.player_characters["target"].hp,
            6.0
        );
    }

    #[test]
    fn quick_cast_uses_text_skill_args_in_rule_text() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 10.0,
                max_mp: 10.0,
                status: CharacterStatus {
                    dex: 4,
                    ..Default::default()
                },
                skill_names: vec!["变量类型".to_owned()],
                skill_notes: vec!["主动使用对目标造成2点伤害类型伤害".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata {
                    args: vec![crate::napcat::SkillPoolArg {
                        name: "伤害类型".to_owned(),
                        kind: "字符串".to_owned(),
                        value: "远程".to_owned(),
                    }],
                    ..Default::default()
                }],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["caster".to_owned(), "target".to_owned()],
            basic_config: TrpgBasicConfig {
                dex_range_damage_bonus: 0.5,
                ..Default::default()
            },
            ..Default::default()
        });
        manager.current_trpg_group = Some("party".to_owned());
        let skill = {
            let caster = manager.player_characters.get_mut("caster").unwrap();
            quick_cast_skills(caster).remove(0)
        };
        let effect = quick_cast_effect(
            &skill.note,
            &skill.arg_values,
            skill.skill_type.as_deref(),
            None,
            &[],
        );

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect,
                cast_turn: 0,
                force: false,
            },
        ));
        assert_eq!(
            manager.player_characters["target"].hp,
            4.0
        );
    }

    #[test]
    fn quick_cast_uses_skill_type_as_default_damage_type() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 10.0,
                max_mp: 10.0,
                status: CharacterStatus {
                    dex: 4,
                    ..Default::default()
                },
                skill_names: vec!["远程伤害".to_owned()],
                skill_notes: vec!["主动使用对目标造成2点伤害".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata {
                    skill_type: Some("远程".to_owned()),
                    ..Default::default()
                }],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["caster".to_owned(), "target".to_owned()],
            basic_config: TrpgBasicConfig {
                dex_range_damage_bonus: 0.5,
                ..Default::default()
            },
            ..Default::default()
        });
        manager.current_trpg_group = Some("party".to_owned());
        let skill = {
            let caster = manager.player_characters.get_mut("caster").unwrap();
            quick_cast_skills(caster).remove(0)
        };
        let effect = quick_cast_effect(
            &skill.note,
            &skill.arg_values,
            skill.skill_type.as_deref(),
            None,
            &[],
        );

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect,
                cast_turn: 0,
                force: false,
            },
        ));
        assert_eq!(
            manager.player_characters["target"].hp,
            4.0
        );
    }

    #[test]
    fn quick_cast_range_damage_uses_converter_magic_bonus_talent() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 10.0,
                max_mp: 10.0,
                status: CharacterStatus {
                    dex: 4,
                    int_: 5,
                    ..Default::default()
                },
                skill_names: vec!["远程伤害".to_owned(), "数魔转换器".to_owned()],
                skill_notes: vec!["主动使用对目标造成2点伤害".to_owned(), String::new()],
                skill_metadata: vec![
                    CharacterSkillMetadata {
                        skill_type: Some("远程".to_owned()),
                        ..Default::default()
                    },
                    CharacterSkillMetadata::talent("normal_talent", "天赋"),
                ],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["caster".to_owned(), "target".to_owned()],
            basic_config: TrpgBasicConfig {
                dex_range_damage_bonus: 0.5,
                int_damage_bonus: 0.2,
                ..Default::default()
            },
            ..Default::default()
        });
        manager.current_trpg_group = Some("party".to_owned());
        let skill = {
            let caster = manager.player_characters.get_mut("caster").unwrap();
            quick_cast_skills(caster).remove(0)
        };
        let effect = quick_cast_effect(
            &skill.note,
            &skill.arg_values,
            skill.skill_type.as_deref(),
            None,
            &[],
        );

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect,
                cast_turn: 0,
                force: false,
            },
        ));
        assert_eq!(
            manager.player_characters["target"].hp,
            2.0
        );
    }

    #[test]
    fn quick_cast_targets_use_metadata_range_when_area_omits_radius() {
        let effect = QuickCastEffect::Damage {
            amount: 1.0,
            target: TargetSelector {
                actor: ActorRef::Target,
                area: Some(crate::rule_engine::AreaSelector {
                    radius_meters: None,
                }),
            },
            damage_type: DamageType::Physical,
        };
        let character_targets = vec![
            ("1".to_owned(), "施法者".to_owned()),
            ("near".to_owned(), "近处".to_owned()),
            ("far".to_owned(), "远处".to_owned()),
        ];
        let scene_positions = SceneCharacterPositions {
            positions: HashMap::from([
                (
                    "near".to_owned(),
                    Vec3::new(2.9, 0.0, 0.0),
                ),
                (
                    "far".to_owned(),
                    Vec3::new(3.1, 0.0, 0.0),
                ),
            ]),
        };
        let camera_positions = ScenePlayerCameraPositions {
            positions: HashMap::from([(1, Vec3::ZERO)]),
        };

        let targets = quick_cast_targets(
            "1",
            &effect,
            &character_targets,
            Some(&scene_positions),
            Some(&camera_positions),
            skill_range_radius(Some(3)),
            None,
        );

        assert_eq!(targets, vec!["near".to_owned()]);
        assert_eq!(
            quick_cast_radius(&effect, Some(3.0)),
            Some(3.0)
        );
    }

    #[test]
    fn quick_cast_single_target_respects_metadata_range() {
        let effect = QuickCastEffect::Damage {
            amount: 1.0,
            target: TargetSelector {
                actor: ActorRef::Target,
                area: None,
            },
            damage_type: DamageType::Physical,
        };
        let character_targets = vec![
            ("1".to_owned(), "施法者".to_owned()),
            ("far".to_owned(), "远处".to_owned()),
        ];
        let scene_positions = SceneCharacterPositions {
            positions: HashMap::from([(
                "far".to_owned(),
                Vec3::new(3.1, 0.0, 0.0),
            )]),
        };
        let camera_positions = ScenePlayerCameraPositions {
            positions: HashMap::from([(1, Vec3::ZERO)]),
        };

        let targets = quick_cast_targets(
            "1",
            &effect,
            &character_targets,
            Some(&scene_positions),
            Some(&camera_positions),
            skill_range_radius(Some(3)),
            None,
        );

        assert!(targets.is_empty());
    }

    #[test]
    fn quick_cast_range_damage_uses_tex30_minimum_range_talent() {
        let caster = PlayerCharacter {
            level: 2,
            skill_names: vec!["瞄准镜Tex-30".to_owned()],
            skill_metadata: vec![CharacterSkillMetadata::talent("normal_talent", "天赋")],
            ..Default::default()
        };
        let effect = QuickCastEffect::Damage {
            amount: 1.0,
            target: TargetSelector {
                actor: ActorRef::Target,
                area: None,
            },
            damage_type: DamageType::Range,
        };
        let character_targets = vec![
            ("1".to_owned(), "施法者".to_owned()),
            ("far".to_owned(), "远处".to_owned()),
        ];
        let scene_positions = SceneCharacterPositions {
            positions: HashMap::from([(
                "far".to_owned(),
                Vec3::new(20.0, 0.0, 0.0),
            )]),
        };
        let camera_positions = ScenePlayerCameraPositions {
            positions: HashMap::from([(1, Vec3::ZERO)]),
        };

        let fallback_radius = quick_cast_skill_range_radius(&caster, &effect, Some(3), None);
        assert_eq!(fallback_radius, Some(30.0));
        let targets = quick_cast_targets(
            "1",
            &effect,
            &character_targets,
            Some(&scene_positions),
            Some(&camera_positions),
            fallback_radius,
            None,
        );
        assert_eq!(targets, vec!["far".to_owned()]);

        let physical_effect = QuickCastEffect::Damage {
            amount: 1.0,
            target: TargetSelector {
                actor: ActorRef::Target,
                area: None,
            },
            damage_type: DamageType::Physical,
        };
        assert_eq!(
            quick_cast_skill_range_radius(&caster, &physical_effect, Some(3), None),
            Some(3.0)
        );
        let targets = quick_cast_targets(
            "1",
            &physical_effect,
            &character_targets,
            Some(&scene_positions),
            Some(&camera_positions),
            quick_cast_skill_range_radius(&caster, &physical_effect, Some(3), None),
            None,
        );
        assert!(targets.is_empty());
    }

    #[test]
    fn quick_cast_spell_skill_uses_magic_web_range_talent() {
        let caster = PlayerCharacter {
            skill_names: vec!["魔网延伸".to_owned()],
            skill_metadata: vec![CharacterSkillMetadata::talent("normal_talent", "天赋")],
            ..Default::default()
        };
        let effect = QuickCastEffect::Damage {
            amount: 1.0,
            target: TargetSelector {
                actor: ActorRef::Target,
                area: None,
            },
            damage_type: DamageType::Magical,
        };
        let character_targets = vec![
            ("1".to_owned(), "施法者".to_owned()),
            ("far".to_owned(), "远处".to_owned()),
        ];
        let scene_positions = SceneCharacterPositions {
            positions: HashMap::from([(
                "far".to_owned(),
                Vec3::new(10.4, 0.0, 0.0),
            )]),
        };
        let camera_positions = ScenePlayerCameraPositions {
            positions: HashMap::from([(1, Vec3::ZERO)]),
        };

        let fallback_radius =
            quick_cast_skill_range_radius(&caster, &effect, Some(10), Some("法术"));
        assert_eq!(fallback_radius, Some(10.5));
        let targets = quick_cast_targets(
            "1",
            &effect,
            &character_targets,
            Some(&scene_positions),
            Some(&camera_positions),
            fallback_radius,
            None,
        );
        assert_eq!(targets, vec!["far".to_owned()]);

        let targets = quick_cast_targets(
            "1",
            &effect,
            &character_targets,
            Some(&scene_positions),
            Some(&camera_positions),
            quick_cast_skill_range_radius(&caster, &effect, Some(10), None),
            None,
        );
        assert!(targets.is_empty());
    }

    #[test]
    fn quick_cast_range_target_class_expands_single_target_rule() {
        let effect = QuickCastEffect::Damage {
            amount: 1.0,
            target: TargetSelector {
                actor: ActorRef::Target,
                area: None,
            },
            damage_type: DamageType::Physical,
        };
        let character_targets = vec![
            ("1".to_owned(), "施法者".to_owned()),
            ("near".to_owned(), "近处".to_owned()),
            ("far".to_owned(), "远处".to_owned()),
        ];
        let scene_positions = SceneCharacterPositions {
            positions: HashMap::from([
                (
                    "near".to_owned(),
                    Vec3::new(2.9, 0.0, 0.0),
                ),
                (
                    "far".to_owned(),
                    Vec3::new(3.1, 0.0, 0.0),
                ),
            ]),
        };
        let camera_positions = ScenePlayerCameraPositions {
            positions: HashMap::from([(1, Vec3::ZERO)]),
        };

        let targets = quick_cast_targets(
            "1",
            &effect,
            &character_targets,
            Some(&scene_positions),
            Some(&camera_positions),
            skill_range_radius(Some(3)),
            Some("范围"),
        );

        assert_eq!(targets, vec!["near".to_owned()]);
    }

    #[test]
    fn quick_cast_applies_group_attribute_and_combat_modifiers() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                mp: 10.0,
                max_mp: 10.0,
                status: CharacterStatus {
                    str_: 4,
                    agi: 51,
                    dex: 3,
                    int_: 5,
                    wis: 2,
                    ..Default::default()
                },
                damage_dealt_modifier: 2.0,
                healing_dealt_modifier: 2.0,
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 20.0,
                max_hp: 20.0,
                damage_taken_modifier: 0.5,
                healing_taken_modifier: 0.5,
                ..Default::default()
            });
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["caster".to_owned(), "target".to_owned()],
            basic_config: TrpgBasicConfig {
                str_damage_bonus: 0.25,
                agi_damage_bonus: 0.5,
                dex_damage_bonus: 0.1,
                int_heal_bonus: 0.1,
                wis_heal_bonus: 0.2,
                ..Default::default()
            },
            ..Default::default()
        });
        manager.current_trpg_group = Some("party".to_owned());
        let skill = QuickCastSkill {
            index: 0,
            name: "测试".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill: skill.clone(),
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Damage {
                    amount: 2.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                    damage_type: DamageType::Physical,
                }),
                cast_turn: 0,
                force: false,
            },
        ));
        assert!((manager.player_characters["target"].hp - 14.4).abs() < 0.0001);
        assert!((manager.player_characters["target"].damage_taken_this_turn - 5.6).abs() < 0.0001);

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Heal {
                    amount: 1.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                }),
                cast_turn: 0,
                force: false,
            },
        ));
        assert!((manager.player_characters["target"].hp - 16.3).abs() < 0.0001);
        assert!((manager.player_characters["target"].healing_taken_this_turn - 1.9).abs() < 0.0001);
    }

    #[test]
    fn quick_cast_applies_typed_damage_taken_talents() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 20.0,
                max_hp: 20.0,
                skill_names: vec!["人类基因工程".to_owned(), "抗魔体质".to_owned()],
                skill_metadata: vec![
                    CharacterSkillMetadata::talent("normal_talent", "天赋"),
                    CharacterSkillMetadata::talent("support_talent", "辅助天赋"),
                ],
                ..Default::default()
            });
        let skill = QuickCastSkill {
            index: 0,
            name: "测试".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill: skill.clone(),
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Damage {
                    amount: 10.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                    damage_type: DamageType::Diseased,
                }),
                cast_turn: 0,
                force: false,
            },
        ));
        assert!((manager.player_characters["target"].hp - 11.5).abs() < 0.0001);

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Damage {
                    amount: 10.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                    damage_type: DamageType::Magical,
                }),
                cast_turn: 0,
                force: false,
            },
        ));
        assert!((manager.player_characters["target"].hp - 2.5).abs() < 0.0001);
        assert!((manager.player_characters["target"].damage_taken_this_turn - 17.5).abs() < 0.0001);
    }

    #[test]
    fn quick_cast_applies_wound_talent_healing_taken_debuff() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                skill_names: vec!["溃伤".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata::talent("normal_talent", "天赋")],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 20.0,
                max_hp: 20.0,
                ..Default::default()
            });
        let skill = QuickCastSkill {
            index: 0,
            name: "测试".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Damage {
                    amount: 5.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                    damage_type: DamageType::Physical,
                }),
                cast_turn: 0,
                force: false,
            },
        ));

        let target = &manager.player_characters["target"];
        assert!((target.hp - 15.0).abs() < 0.0001);
        assert_eq!(target.active_buffs.len(), 1);
        assert_eq!(target.active_buffs[0].name, "溃伤");
        assert_eq!(
            target.active_buffs[0].turns_remaining,
            1
        );

        let mut rule_engine_state = RuleEngineState::default();
        let config = TrpgBasicConfig::default();
        let target = manager.player_characters.get_mut("target").unwrap();
        sync_character_buffs(
            "target",
            target,
            &config,
            &mut rule_engine_state,
            &[],
        );
        assert!((target.healing_taken_modifier - 0.75).abs() < 0.0001);
    }

    #[test]
    fn quick_cast_physical_overkill_scales_lifesteal_and_followup() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 9.0,
                max_hp: 10.0,
                skill_names: vec!["禅宗古训".to_owned(), "苏萨斯之爪".to_owned()],
                skill_metadata: vec![
                    CharacterSkillMetadata::talent("normal_talent", "天赋"),
                    CharacterSkillMetadata::talent("normal_talent", "天赋"),
                ],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 2.0,
                max_hp: 20.0,
                ..Default::default()
            });
        let skill = QuickCastSkill {
            index: 0,
            name: "测试".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Damage {
                    amount: 4.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                    damage_type: DamageType::Physical,
                }),
                cast_turn: 0,
                force: false,
            },
        ));

        let caster = &manager.player_characters["caster"];
        assert!((caster.hp - 9.3).abs() < 0.0001);
        assert!((caster.healing_taken_this_turn - 0.3).abs() < 0.0001);
        let target = &manager.player_characters["target"];
        assert_eq!(target.hp, 0.0);
        assert!((target.damage_taken_this_turn - 2.0).abs() < 0.0001);
        assert!(matches!(
            target.active_buffs[0].tick_actions.as_slice(),
            [BuffTickAction::FixedDamage { amount, damage_type: DamageType::Magical }]
                if (*amount - 0.7).abs() < 0.0001
        ));
    }

    #[test]
    fn quick_cast_physical_damage_schedules_sousas_claw_followup() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                skill_names: vec!["苏萨斯之爪".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata::talent("normal_talent", "天赋")],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 20.0,
                max_hp: 20.0,
                ..Default::default()
            });
        let skill = QuickCastSkill {
            index: 0,
            name: "测试".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Damage {
                    amount: 10.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                    damage_type: DamageType::Physical,
                }),
                cast_turn: 0,
                force: false,
            },
        ));

        let target = &manager.player_characters["target"];
        assert!((target.hp - 10.0).abs() < 0.0001);
        assert_eq!(target.active_buffs.len(), 1);
        assert_eq!(
            target.active_buffs[0].name,
            "苏萨斯之爪"
        );

        let mut rule_engine_state = RuleEngineState::default();
        assert!(advance_buffs_for_players(
            &mut manager,
            &["target".to_owned()],
            &mut rule_engine_state,
        ));
        let target = &manager.player_characters["target"];
        assert!((target.hp - 6.5).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 13.5).abs() < 0.0001);
        assert_eq!(target.active_buffs.len(), 1);

        assert!(advance_buffs_for_players(
            &mut manager,
            &["target".to_owned()],
            &mut rule_engine_state,
        ));
        assert!(manager.player_characters["target"].active_buffs.is_empty());
    }

    #[test]
    fn quick_cast_stacked_buffs_recompute_from_original_base_stats() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 100.0,
                max_hp: 100.0,
                ..Default::default()
            });
        let skill = |index| QuickCastSkill {
            index,
            name: format!("测试{index}"),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        let target_selector = TargetSelector::single(ActorRef::Target);
        let max_hp_buff = crate::rule_engine::RuleBuffTemplate {
            name: "巨人之力".to_owned(),
            kind: BuffKind::Magic,
            priority: 0,
            turns_remaining: 3,
            beneficial: true,
            effects: vec![BuffEffect {
                field: BuffField::MaxHp,
                value: BuffValue::AddPercent(100.0),
            }],
            tick_actions: Vec::new(),
        };
        let guard_buff = crate::rule_engine::RuleBuffTemplate {
            name: "守护".to_owned(),
            kind: BuffKind::Magic,
            priority: 1,
            turns_remaining: 3,
            beneficial: true,
            effects: vec![BuffEffect {
                field: BuffField::DamageTakenModifier,
                value: BuffValue::AddPercent(-50.0),
            }],
            tick_actions: Vec::new(),
        };
        let mut rule_engine_state = RuleEngineState::default();
        let config = TrpgBasicConfig::default();

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill: skill(0),
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::GrantBuff {
                    target: target_selector,
                    buff: max_hp_buff,
                }),
                cast_turn: 0,
                force: false,
            },
        ));
        sync_character_buffs(
            "target",
            manager.player_characters.get_mut("target").unwrap(),
            &config,
            &mut rule_engine_state,
            &[],
        );
        assert!((manager.player_characters["target"].max_hp - 200.0).abs() < 0.0001);

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill: skill(1),
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::GrantBuff {
                    target: target_selector,
                    buff: guard_buff,
                }),
                cast_turn: 0,
                force: false,
            },
        ));
        sync_character_buffs(
            "target",
            manager.player_characters.get_mut("target").unwrap(),
            &config,
            &mut rule_engine_state,
            &[],
        );

        let target = &manager.player_characters["target"];
        assert!((target.max_hp - 200.0).abs() < 0.0001);
        assert!((target.damage_taken_modifier - 0.5).abs() < 0.0001);
        assert!((target.buff_base_stats.as_ref().unwrap().max_hp - 100.0).abs() < 0.0001);
    }

    #[test]
    fn quick_cast_applies_large_hit_damage_reduction_talent() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 20.0,
                max_hp: 20.0,
                skill_names: vec!["过度免疫".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata::talent("support_talent", "辅助天赋")],
                ..Default::default()
            });
        let skill = QuickCastSkill {
            index: 0,
            name: "测试".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Damage {
                    amount: 5.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                    damage_type: DamageType::Physical,
                }),
                cast_turn: 0,
                force: false,
            },
        ));

        let target = &manager.player_characters["target"];
        assert!((target.hp - 16.0).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 4.0).abs() < 0.0001);
    }

    #[test]
    fn quick_cast_applies_minimum_damage_floor_talent() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                level: 4,
                skill_names: vec!["菜鸡猛啄".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata::talent("normal_talent", "天赋")],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 20.0,
                max_hp: 20.0,
                damage_taken_modifier: 0.1,
                ..Default::default()
            });
        let skill = QuickCastSkill {
            index: 0,
            name: "测试".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Damage {
                    amount: 2.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                    damage_type: DamageType::Physical,
                }),
                cast_turn: 0,
                force: false,
            },
        ));

        let target = &manager.player_characters["target"];
        assert!((target.hp - 16.0).abs() < 0.0001);
        assert!((target.damage_taken_this_turn - 4.0).abs() < 0.0001);
    }

    #[test]
    fn quick_cast_applies_chaos_output_variance_talent() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 100.0,
                max_hp: 100.0,
                skill_names: vec!["混沌无序".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata::talent("normal_talent", "天赋")],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 50.0,
                max_hp: 100.0,
                ..Default::default()
            });
        let skill = QuickCastSkill {
            index: 0,
            name: "测试混沌".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill: skill.clone(),
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Damage {
                    amount: 10.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                    damage_type: DamageType::Physical,
                }),
                cast_turn: 0,
                force: false,
            },
        ));
        let target = &manager.player_characters["target"];
        assert!(
            (8.5..=11.5).contains(&target.damage_taken_this_turn),
            "damage roll out of range: {}",
            target.damage_taken_this_turn
        );

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Heal {
                    amount: 10.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                }),
                cast_turn: 1,
                force: false,
            },
        ));
        let target = &manager.player_characters["target"];
        assert!(
            (8.5..=11.5).contains(&target.healing_taken_this_turn),
            "healing roll out of range: {}",
            target.healing_taken_this_turn
        );
    }

    #[test]
    fn quick_cast_applies_dying_target_healing_talent() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 10.0,
                skill_names: vec!["生死时速".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata::talent("support_talent", "辅助天赋")],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 4.0,
                max_hp: 20.0,
                ..Default::default()
            });
        let skill = QuickCastSkill {
            index: 0,
            name: "测试治疗".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Heal {
                    amount: 4.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                }),
                cast_turn: 0,
                force: false,
            },
        ));

        let target = &manager.player_characters["target"];
        assert!((target.hp - 10.0).abs() < 0.0001);
        assert!((target.healing_taken_this_turn - 6.0).abs() < 0.0001);
    }

    #[test]
    fn quick_cast_applies_wounded_healing_dealt_talent() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 16.0,
                max_hp: 20.0,
                skill_names: vec!["火源之力".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata::talent("support_talent", "辅助天赋")],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 0.0,
                max_hp: 30.0,
                ..Default::default()
            });
        let skill = QuickCastSkill {
            index: 0,
            name: "测试治疗".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Heal {
                    amount: 10.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                }),
                cast_turn: 0,
                force: false,
            },
        ));

        let target = &manager.player_characters["target"];
        assert!((target.hp - 12.0).abs() < 0.0001);
        assert!((target.healing_taken_this_turn - 12.0).abs() < 0.0001);
    }

    #[test]
    fn quick_cast_applies_mutual_aid_healing_feedback_talent() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 10.0,
                max_hp: 20.0,
                skill_names: vec!["互帮互助".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata::talent("support_talent", "辅助天赋")],
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 18.0,
                max_hp: 20.0,
                skill_names: vec!["互帮互助".to_owned()],
                skill_metadata: vec![CharacterSkillMetadata::talent("support_talent", "辅助天赋")],
                ..Default::default()
            });
        let skill = QuickCastSkill {
            index: 0,
            name: "互助治疗".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Heal {
                    amount: 4.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                }),
                cast_turn: 0,
                force: false,
            },
        ));

        let caster = &manager.player_characters["caster"];
        assert!((caster.hp - 12.0).abs() < 0.0001);
        assert!((caster.healing_taken_this_turn - 2.0).abs() < 0.0001);
        let target = &manager.player_characters["target"];
        assert!((target.hp - 20.0).abs() < 0.0001);
        assert!((target.healing_taken_this_turn - 2.0).abs() < 0.0001);
    }

    #[test]
    fn group_world_turn_resets_character_turn_totals() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                damage_taken_this_turn: 6.0,
                healing_taken_this_turn: 2.0,
                ..Default::default()
            });
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["target".to_owned()],
            ..Default::default()
        });
        let mut rule_engine_state = RuleEngineState::default();

        assert!(advance_group_world_turn(
            &mut manager,
            "party",
            &mut rule_engine_state
        ));

        let target = &manager.player_characters["target"];
        assert_eq!(target.damage_taken_this_turn, 0.0);
        assert_eq!(target.healing_taken_this_turn, 0.0);
    }

    #[test]
    fn group_restores_the_persisted_pre_first_turn_player_stats() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                name: "原角色名".to_owned(),
                hp: 18.0,
                max_hp: 24.0,
                mp: 7.0,
                max_mp: 12.0,
                level: 3,
                exp: 4,
                damage_taken_this_turn: 2.0,
                ..Default::default()
            });
        manager.trpg_groups.insert("party".to_owned(), TrpgGroup {
            players: vec!["target".to_owned()],
            ..Default::default()
        });
        let mut rule_engine_state = RuleEngineState::default();
        assert!(advance_group_world_turn(
            &mut manager,
            "party",
            &mut rule_engine_state,
        ));
        assert_eq!(
            manager.trpg_groups["party"].initial_player_states["target"].hp,
            18.0
        );
        {
            let character = manager.player_characters.get_mut("target").unwrap();
            character.name = "后来改名".to_owned();
            character.hp = 1.0;
            character.max_hp = 30.0;
            character.mp = 0.0;
            character.level = 9;
            character.exp = 99;
        }

        assert_eq!(
            restore_group_initial_player_stats(
                &mut manager,
                "party",
                &mut rule_engine_state,
            ),
            (1, 0)
        );

        let character = &manager.player_characters["target"];
        assert_eq!(character.name, "后来改名");
        assert_eq!(character.hp, 18.0);
        assert_eq!(character.max_hp, 24.0);
        assert_eq!(character.mp, 7.0);
        assert_eq!(character.level, 3);
        assert_eq!(character.exp, 4);
        assert_eq!(character.damage_taken_this_turn, 2.0);
    }

    #[test]
    fn quick_cast_applies_low_hp_damage_penalty() {
        let mut manager = empty_manager();
        manager
            .player_characters
            .insert("caster".to_owned(), PlayerCharacter {
                hp: 5.0,
                max_hp: 10.0,
                mp: 10.0,
                max_mp: 10.0,
                ..Default::default()
            });
        manager
            .player_characters
            .insert("target".to_owned(), PlayerCharacter {
                hp: 20.0,
                max_hp: 20.0,
                ..Default::default()
            });
        let skill = QuickCastSkill {
            index: 0,
            name: "测试".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };

        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Damage {
                    amount: 4.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                    damage_type: DamageType::Physical,
                }),
                cast_turn: 0,
                force: false,
            },
        ));
        assert_eq!(
            manager.player_characters["target"].hp,
            17.0
        );

        manager.player_characters.get_mut("target").unwrap().hp = 20.0;
        let caster = manager.player_characters.get_mut("caster").unwrap();
        caster.skill_names.push("疲惫行者".to_owned());
        caster.skill_metadata.push(CharacterSkillMetadata::talent(
            "normal_talent",
            "天赋",
        ));
        let skill = QuickCastSkill {
            index: 0,
            name: "疲惫行者测试".to_owned(),
            note: String::new(),
            skill_type: None,
            legacy_buff_machine_json: None,
            mp_cost: 0.0,
            cooldown_turns: 0,
            cooldown_left: None,
            target_count: None,
            target_class: None,
            range: None,
            arg_values: SkillRuleArgs::default(),
        };
        assert!(apply_quick_cast_action_to_manager(
            &mut manager,
            QuickCastAction {
                caster_id: "caster".to_owned(),
                skill,
                targets: vec!["target".to_owned()],
                effect: Some(QuickCastEffect::Damage {
                    amount: 4.0,
                    target: TargetSelector {
                        actor: ActorRef::Target,
                        area: None,
                    },
                    damage_type: DamageType::Physical,
                }),
                cast_turn: 1,
                force: false,
            },
        ));
        assert!((manager.player_characters["target"].hp - 16.8).abs() < 0.0001);
    }
}
