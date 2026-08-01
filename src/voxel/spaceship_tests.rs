use std::collections::HashMap;

use super::*;

#[test]
fn small_spaceships_have_shaped_walkable_decorated_cabins() {
    for variant in 0..SMALL_SPACESHIP_COUNT {
        let cells = small_spaceship_voxel_cells(variant)
            .into_iter()
            .collect::<HashMap<_, _>>();
        let half_width = 5 + (variant % 2) as i32;
        let nose = -13 - (variant % 3) as i32;
        let tail = 9 + (variant % 2) as i32;
        let wing_span = half_width + 4 + (variant % 3 == 2) as i32;

        // Three cells wide, four cells high, and eight cells long: enough room
        // for the canonical first-person body to walk and look around.
        for z in -3..=4 {
            for x in -1..=1 {
                assert!(
                    cells
                        .get(&IVec3::new(x, 0, z))
                        .is_some_and(TrpgVoxelConnector::solid),
                    "variant {variant} cabin needs a solid floor at ({x}, {z})"
                );
                assert!(
                    cells
                        .get(&IVec3::new(x, 5, z))
                        .is_some_and(TrpgVoxelConnector::solid),
                    "variant {variant} cabin needs a solid roof at ({x}, {z})"
                );
                for y in 1..=4 {
                    assert!(
                        !cells.contains_key(&IVec3::new(x, y, z)),
                        "variant {variant} cabin air was blocked at ({x}, {y}, {z})"
                    );
                }
            }
        }

        assert_eq!(
            cells.get(&IVec3::new(0, 1, -5)),
            Some(&10)
        );
        assert_eq!(
            cells.get(&IVec3::new(0, 2, -5)),
            Some(&8)
        );
        assert_eq!(
            cells.get(&IVec3::new(half_width - 1, 1, -2)),
            Some(&8)
        );
        assert_eq!(
            cells.get(&IVec3::new(-(half_width - 1), 1, 4)),
            Some(&10)
        );

        // The aft opening and ramp make the room enterable rather than a
        // sealed pocket inside the collider.
        for x in -1..=1 {
            for y in 1..=3 {
                assert!(!cells.contains_key(&IVec3::new(x, y, tail)));
            }
            assert!(cells.contains_key(&IVec3::new(x, 0, tail + 1)));
        }

        let nose_floor_width = cells
            .keys()
            .filter(|cell| cell.y == 0 && cell.z == nose)
            .count();
        let cabin_floor_width = cells
            .keys()
            .filter(|cell| cell.y == 0 && cell.z == -5)
            .count();
        assert!(nose_floor_width < cabin_floor_width);
        assert!(cells.contains_key(&IVec3::new(wing_span, 0, 1)));
    }
}

#[test]
fn spaceship_takeover_item_uses_the_assigned_pilot_identity() {
    let mut editor = VoxelEditorState::default();
    editor.put_in_selected_hotbar(VoxelCreativeItem::SpaceshipPossessionTool);
    assert!(editor.is_spaceship_possession_tool_equipped());
    assert_eq!(editor.active_tool_label(), "舰船接管器");
    assert!(spaceship_possession_tool_can_target(
        true, None
    ));
    assert!(!spaceship_possession_tool_can_target(
        true,
        Some(42)
    ));

    let mut possession = VoxelPossessionState::default();
    let mut control = VoxelSpaceshipControlState::default();
    begin_voxel_spaceship_takeover(
        "small-ship-01",
        Some(42),
        &mut editor,
        &mut possession,
        &mut control,
    );

    assert_eq!(possession.active_user_id, Some(42));
    assert_eq!(
        control.driving_ship_id.as_deref(),
        Some("small-ship-01")
    );
    assert_eq!(
        control.selected_ship_id.as_deref(),
        Some("small-ship-01")
    );
    assert!(editor.first_person_enabled);
    assert!(editor.first_person_cursor_released);

    begin_voxel_spaceship_takeover(
        "small-ship-02",
        None,
        &mut editor,
        &mut possession,
        &mut control,
    );
    assert_eq!(possession.active_user_id, None);
    assert_eq!(
        control.driving_ship_id.as_deref(),
        Some("small-ship-02")
    );
}
