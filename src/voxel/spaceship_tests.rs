use std::collections::{
    HashMap,
    HashSet,
};

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
fn medium_spaceship_has_a_larger_walkable_furnished_cabin_and_aft_ramp() {
    let cells = medium_spaceship_voxel_cells()
        .into_iter()
        .collect::<HashMap<_, _>>();

    for z in -7..=10 {
        for x in -2..=2 {
            assert!(
                cells
                    .get(&IVec3::new(x, 0, z))
                    .is_some_and(TrpgVoxelConnector::solid),
                "medium cabin needs a solid floor at ({x}, {z})"
            );
            assert!(
                cells
                    .get(&IVec3::new(x, 8, z))
                    .is_some_and(TrpgVoxelConnector::solid),
                "medium cabin needs a solid roof at ({x}, {z})"
            );
            for y in 1..=7 {
                assert!(
                    !cells.contains_key(&IVec3::new(x, y, z)),
                    "medium cabin air was blocked at ({x}, {y}, {z})"
                );
            }
        }
    }

    assert_eq!(
        cells.get(&IVec3::new(0, 1, -12)),
        Some(&10)
    );
    assert_eq!(
        cells.get(&IVec3::new(0, 2, -12)),
        Some(&8)
    );
    assert_eq!(
        cells.get(&IVec3::new(3, 1, -8)),
        Some(&9)
    );
    assert_eq!(
        cells.get(&IVec3::new(13, 2, 6)),
        Some(&7)
    );
    assert!(cells.contains_key(&IVec3::new(18, 0, 2)));

    for x in -2..=2 {
        for y in 1..=7 {
            assert!(!cells.contains_key(&IVec3::new(x, y, 14)));
        }
        assert!(cells.contains_key(&IVec3::new(x, 0, 19)));
    }

    let small_cell_count = small_spaceship_voxel_cells(0).len();
    assert!(cells.len() > small_cell_count * 2);
}

fn hangar_parked_cell(cell: IVec3, berth: IVec3) -> IVec3 {
    berth + IVec3::new(-cell.x, cell.y, -cell.z)
}

#[test]
fn arrogance_hangar_holds_the_fleet_and_each_ship_has_a_clear_launch_sweep() {
    let carrier = combat_spaceship_voxel_cells()
        .into_iter()
        .filter_map(|(cell, material)| TrpgVoxelConnector::solid(&material).then_some(cell))
        .collect::<HashSet<_>>();
    let mut parked = Vec::with_capacity(SMALL_SPACESHIP_COUNT + 1);
    parked.push((
        MEDIUM_SPACESHIP_ID,
        medium_spaceship_voxel_cells(),
        IVec3::new(0, HANGAR_PARKING_Y, HANGAR_PARKING_Z),
    ));
    for (variant, berth_x) in SMALL_SPACESHIP_BERTH_X.into_iter().enumerate() {
        parked.push((
            "small ship",
            small_spaceship_voxel_cells(variant),
            IVec3::new(
                berth_x,
                HANGAR_PARKING_Y,
                HANGAR_PARKING_Z,
            ),
        ));
    }

    let mut fleet_cells = HashSet::new();
    for (name, cells, berth) in parked {
        let solid_cells = cells
            .into_iter()
            .filter_map(|(cell, material)| TrpgVoxelConnector::solid(&material).then_some(cell))
            .collect::<Vec<_>>();
        let world_cells = solid_cells
            .iter()
            .copied()
            .map(|cell| hangar_parked_cell(cell, berth))
            .collect::<Vec<_>>();
        let min_z = world_cells.iter().map(|cell| cell.z).min().unwrap();

        for cell in &world_cells {
            assert!(
                !carrier.contains(cell),
                "{name} overlaps the carrier at {cell:?}"
            );
            assert!(
                fleet_cells.insert(*cell),
                "{name} overlaps another parked ship at {cell:?}"
            );
        }

        // Sweep every real occupied cell forward until the back of the vessel
        // is beyond the deck edge. This proves the opening is flyable, rather
        // than merely checking a center line through the launch mouth.
        for distance in 0..=HANGAR_MOUTH_Z + 1 - min_z {
            for cell in &world_cells {
                let swept = *cell + IVec3::Z * distance;
                assert!(
                    !carrier.contains(&swept),
                    "{name} launch is blocked at {swept:?} after {distance} cells"
                );
            }
        }
    }

    assert_eq!(
        HANGAR_MIN_X,
        -((ARROGANCE.width as i32 - 1) / 2)
    );
    assert_eq!(
        HANGAR_MAX_X,
        HANGAR_MIN_X + ARROGANCE.width as i32 - 1
    );
    assert!(carrier.contains(&IVec3::new(0, 0, HANGAR_MOUTH_Z)));
    assert!(carrier.contains(&IVec3::new(
        0,
        HANGAR_CEILING_Y,
        HANGAR_MOUTH_Z,
    )));
    for y in 1..HANGAR_CEILING_Y - 1 {
        assert!(!carrier.contains(&IVec3::new(0, y, HANGAR_MOUTH_Z + 1)));
    }
}

#[test]
fn old_fleet_layout_migrates_into_the_hangar_and_adds_the_medium_ship() {
    fn spawn_migrated_fleet(
        mut commands: Commands,
        mut meshes: ResMut<Assets<Mesh>>,
        materials: Res<VoxelMaterials>,
        mut store: ResMut<Persistent<VoxelSpaceshipStore>>,
    ) {
        spawn_default_voxel_spaceships(
            &mut commands,
            &mut meshes,
            &materials,
            &mut store,
            false,
        );
    }

    let specs = default_voxel_spaceship_specs();
    let mut old_ships = specs
        .iter()
        .filter(|spec| spec.ship.id != MEDIUM_SPACESHIP_ID)
        .map(|spec| persisted_voxel_spaceship_from_spec(spec, None))
        .collect::<Vec<_>>();
    for ship in &mut old_ships {
        ship.translation = [999.0, 999.0, 999.0];
    }
    old_ships[1].pilot_user_id = Some(42);

    let temporary = tempfile::tempdir().unwrap();
    let store = Persistent::<VoxelSpaceshipStore>::builder()
        .name("test_voxel_spaceship_hangar_migration")
        .format(StorageFormat::Toml)
        .path(temporary.path().join("spaceships.toml"))
        .default(VoxelSpaceshipStore {
            ships: old_ships,
            layout_revision: 0,
        })
        .build()
        .unwrap();
    let mut app = App::new();
    app.init_resource::<Assets<Mesh>>()
        .insert_resource(VoxelMaterials {
            handles: std::array::from_fn(|_| Handle::default()),
            planet_ocean: Handle::default(),
        })
        .insert_resource(store)
        .add_systems(Update, spawn_migrated_fleet);

    app.update();

    let migrated = app.world().resource::<Persistent<VoxelSpaceshipStore>>();
    assert_eq!(
        migrated.layout_revision,
        VOXEL_SPACESHIP_LAYOUT_REVISION
    );
    assert_eq!(migrated.ships.len(), specs.len());
    for spec in &specs {
        let persisted = migrated
            .ships
            .iter()
            .find(|ship| ship.id == spec.ship.id)
            .unwrap();
        assert_eq!(
            persisted.translation,
            spec.transform.translation.to_array()
        );
    }
    assert_eq!(
        migrated
            .ships
            .iter()
            .find(|ship| ship.id == "small-ship-01")
            .unwrap()
            .pilot_user_id,
        Some(42)
    );
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
