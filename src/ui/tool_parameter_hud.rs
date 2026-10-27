use egui::{
    vec2,
    Align2,
    Area,
    Color32,
    Context,
    DragValue,
    Frame,
    Id,
    Order,
    Rect,
};

use crate::voxel::VoxelEditorState;

use crate::info_span;

pub(super) fn show_tool_parameter_hud(
    ctx: &Context,
    editor: &mut VoxelEditorState,
    viewport: Rect,
) {
    #[cfg(feature = "profiling")]
    let _profile = info_span!("tool_parameter_hud").entered();
    let parameters = editor.tool_parameters();
    if parameters.is_empty() && !editor.is_tool_gun_equipped() {
        return;
    }
    Area::new(Id::new("voxel_tool_parameter_hud"))
        .pivot(Align2::RIGHT_BOTTOM)
        .fixed_pos(viewport.right_bottom() - vec2(16.0, 86.0))
        .order(Order::Foreground)
        .movable(false)
        .interactable(editor.tool_parameter_pointer_enabled())
        .show(ctx, |ui| {
            Frame::new()
                .fill(Color32::from_black_alpha(220))
                .corner_radius(6)
                .inner_margin(10)
                .show(ui, |ui| {
                    ui.strong(format!(
                        "工具参数 · {}",
                        editor.active_tool_label()
                    ));
                    for (index, &parameter) in parameters.iter().enumerate() {
                        ui.horizontal(|ui| {
                            let selected = editor.active_tool_parameter() == Some(parameter);
                            if ui
                                .selectable_label(
                                    selected,
                                    format!("{} · {}", index + 1, parameter.label()),
                                )
                                .clicked()
                            {
                                editor.select_tool_parameter(index);
                            }
                            let mut value = parameter.value(editor);
                            if ui
                                .add(
                                    DragValue::new(&mut value)
                                        .range(parameter.range())
                                        .speed(parameter.step()),
                                )
                                .changed()
                            {
                                parameter.set(editor, value);
                            }
                        });
                    }
                    if !parameters.is_empty() {
                        ui.small("Shift + 数字：选择参数 · Shift + 滚轮：调整");
                    } else {
                        ui.small("当前模式没有可调参数");
                    }
                    ui.small("滚轮：切换物品 · Esc：释放鼠标以输入数值");
                    if editor.is_tool_gun_equipped() {
                        ui.small("R：切换工具枪模式");
                    }
                });
        });
}

#[cfg(test)]
mod tests {
    use egui::{
        Pos2,
        RawInput,
        Shape,
    };

    use super::*;
    use crate::voxel::{
        VoxelCreativeItem,
        VoxelEditMode,
    };

    #[test]
    fn tool_parameter_hud_renders_explosion_controls_without_a_toolbar() {
        let ctx = Context::default();
        let viewport = Rect::from_min_size(Pos2::ZERO, vec2(1280.0, 720.0));
        let mut editor = VoxelEditorState::default();
        editor.equip_creative_item(VoxelCreativeItem::Mode(
            VoxelEditMode::Explode,
        ));
        let mut text = String::new();
        for _ in 0..2 {
            ctx.begin_pass(RawInput {
                screen_rect: Some(viewport),
                ..Default::default()
            });
            show_tool_parameter_hud(&ctx, &mut editor, viewport);
            for shape in ctx.end_pass().shapes {
                if let Shape::Text(shape) = shape.shape {
                    text.push_str(&shape.galley.job.text);
                }
            }
        }
        assert!(text.contains("爆炸半径（米）"));
        assert!(text.contains("爆炸冲量"));
        assert!(text.contains("Shift + 数字"));
        assert!(text.contains("Shift + 滚轮"));
    }
}
