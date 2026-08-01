use std::collections::{
    HashMap,
    HashSet,
};

use super::*;

#[test]
fn small_spaceships_have_walkable_panorama_glass_cabins() {
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
            cells.get(&IVec3::new(1, 2, -5)),
            Some(&8)
        );
        assert!(!cells.contains_key(&IVec3::new(0, 2, -5)));
        assert_eq!(
            cells.get(&IVec3::new(half_width - 1, 1, -2)),
            Some(&8)
        );
        assert_eq!(
            cells.get(&IVec3::new(-(half_width - 1), 1, 4)),
            Some(&10)
        );

        // The neutral cockpit sightline stays clear until it reaches a broad
        // forward windscreen, with continuous glass above and on both sides.
        for z in nose + 1..=-2 {
            assert!(
                !cells.contains_key(&IVec3::new(0, 2, z)),
                "variant {variant} cockpit sightline was blocked at z={z}"
            );
        }
        for x in -1..=1 {
            for y in 2..=4 {
                assert_eq!(
                    cells.get(&IVec3::new(x, y, nose)),
                    Some(&VOXEL_GLASS_MATERIAL),
                    "variant {variant} needs forward glass at ({x}, {y}, {nose})"
                );
            }
        }
        for side_x in [-half_width, half_width] {
            for y in 2..=4 {
                assert_eq!(
                    cells.get(&IVec3::new(side_x, y, -4)),
                    Some(&VOXEL_GLASS_MATERIAL),
                    "variant {variant} needs side glass at x={side_x}, y={y}"
                );
            }
        }
        for x in -2..=2 {
            assert_eq!(
                cells.get(&IVec3::new(x, 5, -4)),
                Some(&VOXEL_GLASS_MATERIAL),
                "variant {variant} needs overhead glass at x={x}"
            );
        }
        assert!(
            cells
                .values()
                .filter(|material| **material == VOXEL_GLASS_MATERIAL)
                .count()
                > 80,
            "variant {variant} needs a panoramic amount of glass"
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
fn medium_spaceship_has_a_larger_walkable_panorama_glass_cabin_and_aft_ramp() {
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

    for z in -20..=-5 {
        assert!(
            !cells.contains_key(&IVec3::new(0, 3, z)),
            "medium cockpit sightline was blocked at z={z}"
        );
    }
    for x in -2..=2 {
        for y in 2..=6 {
            assert_eq!(
                cells.get(&IVec3::new(x, y, -21)),
                Some(&VOXEL_GLASS_MATERIAL),
                "medium ship needs forward glass at ({x}, {y})"
            );
        }
    }
    for side_x in [-13, 13] {
        for y in 2..=6 {
            assert_eq!(
                cells.get(&IVec3::new(side_x, y, -10)),
                Some(&VOXEL_GLASS_MATERIAL),
                "medium ship needs side glass at x={side_x}, y={y}"
            );
        }
    }
    for x in -4..=4 {
        assert_eq!(
            cells.get(&IVec3::new(x, 8, -10)),
            Some(&VOXEL_GLASS_MATERIAL),
            "medium ship needs overhead glass at x={x}"
        );
    }

    for x in -2..=2 {
        for y in 1..=7 {
            assert!(!cells.contains_key(&IVec3::new(x, y, 14)));
        }
        assert!(cells.contains_key(&IVec3::new(x, 0, 19)));
    }

    let small_cell_count = small_spaceship_voxel_cells(0).len();
    assert!(cells.len() > small_cell_count * 2);
    let medium_glass_count = cells
        .values()
        .filter(|material| **material == VOXEL_GLASS_MATERIAL)
        .count();
    let small_glass_count = small_spaceship_voxel_cells(0)
        .into_iter()
        .filter(|(_, material)| *material == VOXEL_GLASS_MATERIAL)
        .count();
    assert!(medium_glass_count > small_glass_count * 2);
}

fn hangar_parked_cell(cell: IVec3, berth: IVec3) -> IVec3 {
    berth + IVec3::new(-cell.x, cell.y, -cell.z)
}

#[test]
fn enlarged_arrogance_preserves_its_shape_and_holds_the_fleet_inside() {
    let original = original_combat_spaceship_voxel_cells();
    let scaled_original = scale_combat_spaceship_cells(&original);
    let carrier_cells = combat_spaceship_voxel_cells()
        .into_iter()
        .collect::<HashMap<_, _>>();
    let carrier = carrier_cells
        .iter()
        .filter_map(|(cell, material)| TrpgVoxelConnector::solid(&material).then_some(cell))
        .copied()
        .collect::<HashSet<_>>();

    // The corrected carrier is only the old detailed hull enlarged in place.
    // The hangar is carved out of it; no outer shell or parking box is added.
    assert!(carrier_cells.len() < scaled_original.len());
    for (cell, material) in &carrier_cells {
        assert_eq!(scaled_original.get(cell), Some(material));
    }
    for (cell, material) in &scaled_original {
        if !combat_spaceship_hangar_contains(*cell) {
            assert_eq!(carrier_cells.get(cell), Some(material));
        }
    }
    let scaled_outline = scaled_original
        .keys()
        .map(|cell| (cell.x, cell.z))
        .collect::<HashSet<_>>();
    let carrier_outline = carrier_cells
        .keys()
        .map(|cell| (cell.x, cell.z))
        .collect::<HashSet<_>>();
    assert_eq!(carrier_outline, scaled_outline);

    let original_min = original.keys().copied().reduce(IVec3::min).unwrap();
    let original_max = original.keys().copied().reduce(IVec3::max).unwrap();
    let scaled_min = scaled_original.keys().copied().reduce(IVec3::min).unwrap();
    let scaled_max = scaled_original.keys().copied().reduce(IVec3::max).unwrap();
    assert_eq!(scaled_min, scale_combat_spaceship_cell(original_min));
    assert_eq!(
        scaled_max,
        scale_combat_spaceship_cell(original_max)
            + IVec3::splat(ARROGANCE_SCALE - 1)
    );

    for x in HANGAR_MIN_X..=HANGAR_MAX_X {
        for y in HANGAR_PARKING_Y..HANGAR_CEILING_Y {
            for z in HANGAR_REAR_Z..=HANGAR_MOUTH_Z {
                assert!(
                    !carrier.contains(&IVec3::new(x, y, z)),
                    "the carved hangar contains an internal parking box at {x}, {y}, {z}"
                );
            }
        }
    }

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
        for cell in world_cells
            .iter()
            .filter(|cell| cell.y == HANGAR_PARKING_Y)
        {
            assert!(
                carrier.contains(&IVec3::new(cell.x, HANGAR_PARKING_Y - 1, cell.z)),
                "{name} is not parked on 狂妄号's real deck below {cell:?}"
            );
            assert!(
                carrier.contains(&IVec3::new(cell.x, HANGAR_CEILING_Y, cell.z)),
                "{name} is not inside 狂妄号's real roof below {cell:?}"
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
        ship.docking = None;
    }
    old_ships[1].pilot_user_id = Some(42);

    let temporary = tempfile::tempdir().unwrap();
    let store = Persistent::<VoxelSpaceshipStore>::builder()
        .name("test_voxel_spaceship_hangar_migration")
        .format(StorageFormat::Toml)
        .path(temporary.path().join("spaceships.toml"))
        .default(VoxelSpaceshipStore {
            ships: old_ships,
            layout_revision: 2,
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
        assert_eq!(persisted.docking, spec.docking);
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
fn docked_ships_follow_carrier_launch_with_inertia_and_can_park_again() {
    let specs = default_voxel_spaceship_specs();
    let carrier_ship = specs[0].ship.clone();
    let docked_spec = &specs[1];
    let docking = docked_spec.docking.clone().unwrap();
    let carrier_transform = Transform::from_translation(Vec3::new(31.0, 6.0, -17.0))
        .with_rotation(Quat::from_rotation_y(0.63));
    let carrier_linear = Vec3::new(4.0, -0.5, 2.25);
    let carrier_angular = Vec3::new(0.0, 0.42, 0.0);

    let mut app = App::new();
    app.init_resource::<VoxelSpaceshipControlState>()
        .add_systems(
            Update,
            (
                release_controlled_docked_spaceship,
                dock_idle_voxel_spaceships,
                sync_docked_voxel_spaceships,
            )
                .chain(),
        );
    let carrier = app
        .world_mut()
        .spawn((
            carrier_ship,
            carrier_transform,
            LinearVelocity(carrier_linear),
            AngularVelocity(carrier_angular),
            RigidBody::Dynamic,
        ))
        .id();
    let docked = app
        .world_mut()
        .spawn((
            docked_spec.ship.clone(),
            Transform::IDENTITY,
            LinearVelocity::ZERO,
            AngularVelocity::ZERO,
            RigidBody::Kinematic,
            docking.clone(),
            docked_voxel_spaceship_collision_layers(),
        ))
        .id();

    app.update();

    let expected = docked_voxel_spaceship_world_transform(&carrier_transform, &docking).unwrap();
    let expected_velocity = docked_voxel_spaceship_point_velocity(
        &carrier_transform,
        carrier_linear,
        carrier_angular,
        expected.translation,
    );
    let parked = app.world().entity(docked);
    assert!(parked
        .get::<Transform>()
        .unwrap()
        .translation
        .abs_diff_eq(expected.translation, 0.000_01));
    assert!(parked
        .get::<Transform>()
        .unwrap()
        .rotation
        .abs_diff_eq(expected.rotation, 0.000_01));
    assert!(parked
        .get::<LinearVelocity>()
        .unwrap()
        .0
        .abs_diff_eq(expected_velocity, 0.000_01));
    assert_eq!(
        *parked.get::<RigidBody>().unwrap(),
        RigidBody::Kinematic
    );
    let docked_layers = *parked.get::<CollisionLayers>().unwrap();
    assert!(!docked_layers.interacts_with(carrier_collision_layers()));
    assert!(docked_layers.interacts_with(CollisionLayers::DEFAULT));
    assert!(SpatialQueryFilter::default().test(docked, docked_layers));

    let moved_transform = Transform::from_translation(Vec3::new(-9.0, 11.0, 23.0))
        .with_rotation(Quat::from_rotation_y(-1.1));
    let moved_linear = Vec3::new(-3.0, 1.0, 5.0);
    let moved_angular = Vec3::new(0.0, -0.7, 0.0);
    *app.world_mut()
        .entity_mut(carrier)
        .get_mut::<Transform>()
        .unwrap() = moved_transform;
    app.world_mut()
        .entity_mut(carrier)
        .get_mut::<LinearVelocity>()
        .unwrap()
        .0 = moved_linear;
    app.world_mut()
        .entity_mut(carrier)
        .get_mut::<AngularVelocity>()
        .unwrap()
        .0 = moved_angular;

    app.update();

    let expected_moved =
        docked_voxel_spaceship_world_transform(&moved_transform, &docking).unwrap();
    assert!(app
        .world()
        .entity(docked)
        .get::<Transform>()
        .unwrap()
        .translation
        .abs_diff_eq(expected_moved.translation, 0.000_01));

    app.world_mut()
        .resource_mut::<VoxelSpaceshipControlState>()
        .driving_ship_id = Some(docked_spec.ship.id.clone());
    app.update();

    let expected_launch_velocity = docked_voxel_spaceship_point_velocity(
        &moved_transform,
        moved_linear,
        moved_angular,
        expected_moved.translation,
    );
    let launched = app.world().entity(docked);
    assert_eq!(
        *launched.get::<RigidBody>().unwrap(),
        RigidBody::Dynamic
    );
    assert!(!launched.contains::<VoxelSpaceshipDocked>());
    assert!(!launched.contains::<CollisionLayers>());
    assert!(launched
        .get::<LinearVelocity>()
        .unwrap()
        .0
        .abs_diff_eq(expected_launch_velocity, 0.000_01));
    assert!(launched
        .get::<AngularVelocity>()
        .unwrap()
        .0
        .abs_diff_eq(moved_angular, 0.000_01));

    app.world_mut()
        .resource_mut::<VoxelSpaceshipControlState>()
        .driving_ship_id = None;
    app.update();

    let parked_again = app.world().entity(docked);
    assert_eq!(
        *parked_again.get::<RigidBody>().unwrap(),
        RigidBody::Kinematic
    );
    assert!(parked_again.contains::<VoxelSpaceshipDocked>());
    assert_eq!(
        parked_again.get::<CollisionLayers>().copied(),
        Some(docked_voxel_spaceship_collision_layers())
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
