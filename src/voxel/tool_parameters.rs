use std::ops::RangeInclusive;

use super::{
    VoxelCreativeItem,
    VoxelEditMode,
    VoxelEditorState,
    VoxelLightTool,
    MAX_VOXEL_BRUSH_RADIUS,
    VOXEL_SIZE,
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum VoxelToolParameter {
    BrushRadius,
    ExplosionRadius,
    ExplosionImpulse,
    PushPullImpulse,
    LightIntensity,
    LightRange,
}

impl VoxelToolParameter {
    pub(crate) fn label(self) -> &'static str {
        match self {
            Self::BrushRadius => "笔刷半径（格）",
            Self::ExplosionRadius => "爆炸半径（米）",
            Self::ExplosionImpulse => "爆炸冲量",
            Self::PushPullImpulse => "推拉冲量",
            Self::LightIntensity => "灯光亮度",
            Self::LightRange => "灯光范围（米）",
        }
    }

    pub(crate) fn range(self) -> RangeInclusive<f32> {
        match self {
            Self::BrushRadius => 0.0..=MAX_VOXEL_BRUSH_RADIUS as f32,
            Self::ExplosionRadius | Self::LightRange => VOXEL_SIZE..=100.0,
            Self::ExplosionImpulse => 0.1..=500.0,
            Self::PushPullImpulse => 0.1..=200.0,
            Self::LightIntensity => 0.0..=100_000.0,
        }
    }

    pub(crate) fn step(self) -> f32 {
        match self {
            Self::BrushRadius => 1.0,
            Self::ExplosionRadius | Self::LightRange => VOXEL_SIZE,
            Self::ExplosionImpulse => 0.5,
            Self::PushPullImpulse => 0.25,
            Self::LightIntensity => 100.0,
        }
    }

    pub(crate) fn value(self, editor: &VoxelEditorState) -> f32 {
        match self {
            Self::BrushRadius => editor.brush_radius as f32,
            Self::ExplosionRadius => editor.physics_explosion_radius,
            Self::ExplosionImpulse => editor.physics_explosion_impulse,
            Self::PushPullImpulse => editor.physics_push_pull_impulse,
            Self::LightIntensity => editor.placed_light_intensity,
            Self::LightRange => editor.placed_light_range,
        }
    }

    pub(crate) fn set(self, editor: &mut VoxelEditorState, value: f32) {
        if !value.is_finite() {
            return;
        }
        let range = self.range();
        let value = value.clamp(*range.start(), *range.end());
        match self {
            Self::BrushRadius => editor.brush_radius = value.round() as i32,
            Self::ExplosionRadius => editor.physics_explosion_radius = value,
            Self::ExplosionImpulse => editor.physics_explosion_impulse = value,
            Self::PushPullImpulse => editor.physics_push_pull_impulse = value,
            Self::LightIntensity => editor.placed_light_intensity = value,
            Self::LightRange => editor.placed_light_range = value,
        }
    }
}

impl VoxelEditorState {
    pub(crate) fn tool_parameter_pointer_enabled(&self) -> bool {
        self.first_person_cursor_released || self.creative_inventory_open || self.teleport_menu_open
    }

    pub(crate) fn tool_parameters(&self) -> &'static [VoxelToolParameter] {
        match self.equipped_item {
            Some(VoxelCreativeItem::Material(_)) => &[VoxelToolParameter::BrushRadius],
            Some(VoxelCreativeItem::Light(VoxelLightTool::Remove)) => &[],
            Some(VoxelCreativeItem::Light(_)) => &[
                VoxelToolParameter::LightIntensity,
                VoxelToolParameter::LightRange,
            ],
            Some(VoxelCreativeItem::ToolGun | VoxelCreativeItem::Mode(_)) => match self.mode {
                VoxelEditMode::Add | VoxelEditMode::Remove | VoxelEditMode::Paint => {
                    &[VoxelToolParameter::BrushRadius]
                },
                VoxelEditMode::Push | VoxelEditMode::Pull => &[
                    VoxelToolParameter::PushPullImpulse,
                    VoxelToolParameter::BrushRadius,
                ],
                VoxelEditMode::Explode => &[
                    VoxelToolParameter::ExplosionRadius,
                    VoxelToolParameter::ExplosionImpulse,
                ],
                VoxelEditMode::Physics | VoxelEditMode::Drag => &[],
            },
            _ => &[],
        }
    }

    pub(crate) fn active_tool_parameter(&self) -> Option<VoxelToolParameter> {
        let parameters = self.tool_parameters();
        self.selected_tool_parameter
            .filter(|parameter| parameters.contains(parameter))
            .or_else(|| parameters.first().copied())
    }

    pub(crate) fn select_tool_parameter(&mut self, index: usize) {
        if let Some(parameter) = self.tool_parameters().get(index) {
            self.selected_tool_parameter = Some(*parameter);
        }
    }

    pub(crate) fn adjust_tool_parameter(&mut self, steps: i32) {
        if let Some(parameter) = self.active_tool_parameter() {
            parameter.set(
                self,
                parameter.value(self) + steps as f32 * parameter.step(),
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use bevy::{
        ecs::system::RunSystemOnce,
        input::{
            mouse::{
                MouseMotion,
                MouseScrollUnit,
                MouseWheel,
            },
            touch::TouchPhase,
        },
        prelude::*,
        window::PrimaryWindow,
    };
    use bevy_egui::input::EguiWantsInput;

    use super::*;
    use crate::voxel::{
        control_voxel_camera,
        voxel_editor_shortcuts,
        VoxelPossessionState,
    };

    fn explosion_editor() -> VoxelEditorState {
        let mut editor = VoxelEditorState::default();
        editor.equip_creative_item(VoxelCreativeItem::ToolGun);
        editor.mode = VoxelEditMode::Explode;
        editor.first_person_cursor_released = false;
        editor.first_person_was_enabled = true;
        editor
    }

    #[test]
    fn tool_parameter_selection_bounds_and_mode_changes_use_live_settings() {
        let mut editor = explosion_editor();
        editor.adjust_tool_parameter(1);
        assert_eq!(
            editor.physics_explosion_radius,
            6.0 + VOXEL_SIZE
        );
        editor.select_tool_parameter(1);
        editor.adjust_tool_parameter(2);
        assert_eq!(editor.physics_explosion_impulse, 15.0);
        editor.select_tool_parameter(0);
        editor.adjust_tool_parameter(i32::MIN);
        assert_eq!(
            editor.physics_explosion_radius,
            VOXEL_SIZE
        );
        editor.adjust_tool_parameter(i32::MAX);
        assert_eq!(editor.physics_explosion_radius, 100.0);
        editor.equip_creative_item(VoxelCreativeItem::Material(1));
        editor.adjust_tool_parameter(2);
        assert_eq!(editor.brush_radius, 2);
        editor.equip_creative_item(VoxelCreativeItem::TeleportTool);
        assert!(editor.active_tool_parameter().is_none());
    }

    #[test]
    fn shift_number_selects_parameter_without_equipping_another_item() {
        let mut app = App::new();
        let mut keyboard = ButtonInput::<KeyCode>::default();
        keyboard.press(KeyCode::ShiftLeft);
        keyboard.press(KeyCode::Digit2);
        app.insert_resource(keyboard)
            .insert_resource(explosion_editor())
            .init_resource::<EguiWantsInput>()
            .init_resource::<VoxelPossessionState>();
        app.world_mut()
            .run_system_once(voxel_editor_shortcuts)
            .unwrap();
        let editor = app.world().resource::<VoxelEditorState>();
        assert_eq!(
            editor.active_tool_parameter(),
            Some(VoxelToolParameter::ExplosionImpulse)
        );
        assert_eq!(editor.selected_hotbar_slot, 0);
        assert!(editor.is_tool_gun_equipped());
    }

    #[test]
    fn shift_wheel_adjusts_radius_and_plain_wheel_still_changes_hotbar() {
        for shift in [false, true] {
            let mut app = App::new();
            let mut keyboard = ButtonInput::<KeyCode>::default();
            if shift {
                keyboard.press(KeyCode::ShiftLeft);
            }
            app.insert_resource(keyboard)
                .insert_resource(explosion_editor())
                .init_resource::<ButtonInput<MouseButton>>()
                .init_resource::<Time>()
                .init_resource::<Messages<MouseMotion>>()
                .init_resource::<Messages<MouseWheel>>()
                .init_resource::<EguiWantsInput>()
                .init_resource::<VoxelPossessionState>();
            let window = app
                .world_mut()
                .spawn((
                    Window {
                        focused: true,
                        ..Default::default()
                    },
                    PrimaryWindow,
                ))
                .id();
            app.world_mut()
                .resource_mut::<Messages<MouseWheel>>()
                .write(MouseWheel {
                    unit: MouseScrollUnit::Line,
                    x: 0.0,
                    y: 1.0,
                    window,
                    phase: TouchPhase::Moved,
                });
            app.world_mut()
                .run_system_once(control_voxel_camera)
                .unwrap();
            let editor = app.world().resource::<VoxelEditorState>();
            assert_eq!(
                editor.selected_hotbar_slot,
                if shift { 0 } else { 9 }
            );
            assert_eq!(
                editor.physics_explosion_radius,
                if shift { 6.0 + VOXEL_SIZE } else { 6.0 }
            );
        }
    }
}
