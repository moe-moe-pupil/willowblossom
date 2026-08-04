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
fn arrogance_has_an_enclosed_furnished_cab_at_the_requested_port_bow() {
    let cells = combat_spaceship_voxel_cells()
        .into_iter()
        .collect::<HashMap<_, _>>();

    for x in ARROGANCE_CAB_FRONT_X..=ARROGANCE_CAB_REAR_X {
        let half_depth = combat_spaceship_cab_half_depth(x);
        let floor_y = combat_spaceship_cab_floor_y(x);
        let ceiling_y = combat_spaceship_cab_ceiling_y(x);
        for z in ARROGANCE_CAB_CENTER_Z - half_depth + 1..ARROGANCE_CAB_CENTER_Z + half_depth {
            assert!(
                cells
                    .get(&IVec3::new(x, floor_y, z))
                    .is_some_and(TrpgVoxelConnector::solid),
                "cab needs a solid floor at ({x}, {z})"
            );
            assert!(
                cells
                    .get(&IVec3::new(x, ceiling_y, z))
                    .is_some_and(TrpgVoxelConnector::solid),
                "cab needs a solid ceiling at ({x}, {z})"
            );
        }
    }

    for y in ARROGANCE_CAB_FRONT_FLOOR_Y + 3..=ARROGANCE_CAB_FRONT_CEILING_Y - 3 {
        for z in ARROGANCE_CAB_CENTER_Z - ARROGANCE_CAB_FRONT_HALF_DEPTH
            ..=ARROGANCE_CAB_CENTER_Z + ARROGANCE_CAB_FRONT_HALF_DEPTH
        {
            assert_eq!(
                cells.get(&IVec3::new(ARROGANCE_CAB_FRONT_X, y, z)),
                Some(&VOXEL_GLASS_MATERIAL),
                "cab needs a panoramic forward windscreen at ({z}, {y})"
            );
        }
    }

    assert_eq!(
        combat_spaceship_cab_half_depth(ARROGANCE_CAB_REAR_X),
        22
    );
    assert_eq!(
        combat_spaceship_cab_floor_y(ARROGANCE_CAB_REAR_X),
        0
    );
    assert_eq!(
        combat_spaceship_cab_ceiling_y(ARROGANCE_CAB_REAR_X),
        24
    );

    let eye = default_voxel_spaceship_specs()[0].ship.cockpit_eye_local / VOXEL_SIZE;
    assert!(eye.x < HANGAR_MIN_X as f32);
    assert!(!cells.contains_key(&eye.floor().as_ivec3()));
    for x in ARROGANCE_CAB_FRONT_X + 8..ARROGANCE_CAB_REAR_X {
        for y in combat_spaceship_cab_floor_y(x) + 1..combat_spaceship_cab_ceiling_y(x) {
            assert!(
                !cells.contains_key(&IVec3::new(
                    x,
                    y,
                    ARROGANCE_CAB_CENTER_Z
                )),
                "cab center aisle is blocked at x={x}, y={y}"
            );
        }
    }
}

#[test]
fn arrogance_has_panorama_glass_in_its_workbook_hull_and_bridge() {
    let original = original_combat_spaceship_voxel_cells();
    let hull_glass_columns = original
        .iter()
        .filter(|(_, material)| **material == VOXEL_GLASS_MATERIAL)
        .map(|(cell, _)| (cell.x, cell.z))
        .collect::<HashSet<_>>();

    assert!(
        hull_glass_columns.len() >= 8,
        "Arrogance needs multiple panoramic glass bays in its workbook hull"
    );
    for (x, z) in hull_glass_columns {
        assert_eq!(
            original.get(&IVec3::new(x, 1, z)),
            Some(&6)
        );
        assert_eq!(
            original.get(&IVec3::new(
                x,
                WORKBOOK_ROOM_HEIGHT - 1,
                z
            )),
            Some(&6)
        );
    }

    let bridge_glass_count = combat_spaceship_cab_cells()
        .values()
        .filter(|material| **material == VOXEL_GLASS_MATERIAL)
        .count();
    let finished_glass_count = combat_spaceship_voxel_cells()
        .into_iter()
        .filter(|(_, material)| *material == VOXEL_GLASS_MATERIAL)
        .count();
    assert!(
        finished_glass_count > bridge_glass_count,
        "the finished Arrogance must retain hull glass beyond its bridge"
    );
}

#[test]
fn arrogance_auto_door_trigger_follows_the_moving_ship() {
    let door = combat_spaceship_cab_door();
    let ship_transform = GlobalTransform::from(
        Transform::from_translation(Vec3::new(31.0, -4.0, 17.0))
            .with_rotation(Quat::from_rotation_y(0.73)),
    );
    let player_world = ship_transform.transform_point(door.trigger_center);
    let player_local = voxel_auto_door_player_position(player_world, Some(&ship_transform));

    assert!(player_local.abs_diff_eq(door.trigger_center, 0.000_01));
    assert!(voxel_auto_door_should_open(
        &door,
        player_local
    ));
    assert!(!voxel_auto_door_should_open(
        &door,
        player_world
    ));
}

#[test]
fn arrogance_corridor_auto_doors_fill_surviving_corridors_and_slide_inside() {
    let carrier = combat_spaceship_voxel_cells()
        .into_iter()
        .collect::<HashMap<_, _>>();
    let carrier_solid = carrier
        .iter()
        .filter_map(|(cell, material)| TrpgVoxelConnector::solid(material).then_some(*cell))
        .collect::<HashSet<_>>();
    let doors = combat_spaceship_corridor_auto_doors();

    // The workbook has six corridor doorways; the two that fall inside the
    // fitted cab and the carved hangar lose their surrounding wall, so only
    // four automatic doors remain on the enlarged carrier.
    assert_eq!(doors.len(), 4);
    let door_min_x = doors
        .iter()
        .map(|door| {
            door.cells
                .iter()
                .copied()
                .reduce(IVec3::min)
                .unwrap()
                .x
        })
        .collect::<Vec<_>>();
    assert_eq!(door_min_x, vec![-216, -138, 120, 174]);

    for door in &doors {
        let min = door.cells.iter().copied().reduce(IVec3::min).unwrap();
        let max = door.cells.iter().copied().reduce(IVec3::max).unwrap();
        // The door fills the corridor cross-section: the three-cell-thick wall
        // column, the full corridor width (seven sheet cells scaled by three),
        // and the whole interior height between the floor and the ceiling.
        assert_eq!(max.x - min.x + 1, ARROGANCE_SCALE);
        assert_eq!(max.z - min.z + 1, 7 * ARROGANCE_SCALE);
        assert_eq!(min.y, ARROGANCE_SCALE);
        assert_eq!(max.y, WORKBOOK_ROOM_HEIGHT * ARROGANCE_SCALE - 1);
        assert!(
            door.cells.iter().all(|cell| !carrier_solid.contains(cell)),
            "corridor door aperture is blocked at {min:?}..{max:?}"
        );
        assert!(voxel_auto_door_should_open(
            door,
            door.trigger_center
        ));

        let panels = voxel_auto_door_panels(door);
        assert!(panels.iter().all(|panel| {
            panel.open_translation != panel.closed_translation
                && (panel.open_translation.y - panel.closed_translation.y).abs()
                    < f32::EPSILON
                && panel.trigger_center == door.trigger_center
        }));
        let left_delta = panels[0].open_translation - panels[0].closed_translation;
        let right_delta = panels[1].open_translation - panels[1].closed_translation;
        // The panels split across the corridor width and open left and right
        // into the wall recesses beside the doorway.
        assert!(left_delta.dot(door.width_axis.as_vec3()) < 0.0);
        assert!(right_delta.dot(door.width_axis.as_vec3()) > 0.0);
        for panel in &panels {
            let delta = panel.open_translation - panel.closed_translation;
            let (_, size) = voxel_door_transform_and_size(&panel.cells);
            let panel_width = size.dot(door.width_axis.as_vec3().abs());
            assert!(
                (delta.length() - (panel_width + VOXEL_SIZE * 0.5)).abs() < 0.001,
                "door must slide its width plus the standard clearance"
            );
            let direction = if delta.z < 0.0 { -1 } else { 1 };
            let panel_min = panel.cells.iter().copied().reduce(IVec3::min).unwrap();
            let panel_max = panel.cells.iter().copied().reduce(IVec3::max).unwrap();
            let width_cells = (panel_width / VOXEL_SIZE).round() as i32;
            let shift = direction * (width_cells + 1);
            let open_min = panel_min + IVec3::new(0, 0, shift);
            let open_max = panel_max + IVec3::new(0, 0, shift);
            // The opened leaf clears the entire doorway...
            if direction < 0 {
                assert!(open_max.z < -36);
            } else {
                assert!(open_min.z > -16);
            }
            // ...and the full slid range stays near the hull structure beside
            // the doorway, never floating in deep space beyond it.
            for z in open_min.z..=open_max.z {
                for x in panel_min.x..=panel_max.x {
                    for y in panel_min.y..=panel_max.y {
                        let dest = IVec3::new(x, y, z);
                        let rests_on_hull = (-8..=8).any(|dx| {
                            (-8..=8).any(|dy| {
                                (-8..=8).any(|dz| {
                                    carrier_solid.contains(&(dest + IVec3::new(dx, dy, dz)))
                                })
                            })
                        });
                        assert!(
                            rests_on_hull,
                            "open corridor door floats outside the hull at {dest:?}"
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn arrogance_spawns_bridge_and_corridor_auto_doors() {
    #[derive(Resource, Default)]
    struct TestShipHandle(Option<Entity>);

    fn spawn_ship(
        mut commands: Commands,
        mut meshes: ResMut<Assets<Mesh>>,
        materials: Res<VoxelMaterials>,
        mut handle: ResMut<TestShipHandle>,
        mut spawned: Local<bool>,
    ) {
        if *spawned {
            return;
        }
        *spawned = true;
        let spec = default_voxel_spaceship_specs().remove(0);
        handle.0 = Some(spawn_voxel_spaceship(
            &mut commands,
            &mut meshes,
            &materials,
            &spec,
            Transform::IDENTITY,
            LinearVelocity::ZERO,
            AngularVelocity::ZERO,
            None,
        ));
    }

    let mut app = App::new();
    app.init_resource::<Assets<Mesh>>()
        .insert_resource(VoxelMaterials {
            handles: std::array::from_fn(|_| Handle::default()),
            planet_ocean: Handle::default(),
        })
        .init_resource::<TestShipHandle>()
        .add_systems(Update, spawn_ship);
    app.update();

    // One bridge door and four corridor doors, each split into two panels.
    let mut doors = app
        .world_mut()
        .query_filtered::<Entity, With<VoxelAutoDoor>>();
    assert_eq!(doors.iter(app.world()).count(), 10);
}

#[test]
fn enlarged_arrogance_preserves_its_shape_and_holds_the_fleet_inside() {
    let original = original_combat_spaceship_voxel_cells();
    let scaled_original = scale_combat_spaceship_cells(&original);
    let cab = combat_spaceship_cab_cells();
    let carrier_cells = combat_spaceship_voxel_cells()
        .into_iter()
        .collect::<HashMap<_, _>>();
    let carrier = carrier_cells
        .iter()
        .filter_map(|(cell, material)| TrpgVoxelConnector::solid(&material).then_some(cell))
        .copied()
        .collect::<HashSet<_>>();

    // The carrier remains the enlarged workbook hull, with its hangar and old
    // three-wall port enclosure removed and a connected bow cab fitted there.
    for (cell, material) in &carrier_cells {
        assert!(
            scaled_original.get(cell) == Some(material) || cab.get(cell) == Some(material),
            "carrier gained an unrelated cell at {cell:?}"
        );
    }
    for (cell, material) in &scaled_original {
        if !combat_spaceship_hangar_contains(*cell)
            && !combat_spaceship_obsolete_port_bow_wall_contains(*cell)
            && !combat_spaceship_cab_interior_contains(*cell)
        {
            assert_eq!(
                carrier_cells.get(cell),
                cab.get(cell).or(Some(material)),
                "carrier lost workbook hull outside the fitted cab at {cell:?}"
            );
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
    let cab_outline = cab
        .keys()
        .map(|cell| (cell.x, cell.z))
        .collect::<HashSet<_>>();
    assert!(scaled_outline
        .difference(&carrier_outline)
        .all(|(x, z)| { combat_spaceship_obsolete_port_bow_wall_contains(IVec3::new(*x, 0, *z)) }));
    assert!(carrier_outline
        .difference(&scaled_outline)
        .all(|cell| cab_outline.contains(cell)));

    for cell in scaled_original
        .keys()
        .filter(|cell| combat_spaceship_obsolete_port_bow_wall_contains(**cell))
        .filter(|cell| !cab.contains_key(*cell))
    {
        assert!(
            !carrier.contains(cell),
            "obsolete port enclosure wall remains at {cell:?}"
        );
    }

    let mut carved_hull = scaled_original.clone();
    carve_combat_spaceship_hangar(&mut carved_hull);
    carved_hull.retain(|cell, _| !combat_spaceship_cab_interior_contains(*cell));
    let directions = [
        IVec3::X,
        IVec3::NEG_X,
        IVec3::Y,
        IVec3::NEG_Y,
        IVec3::Z,
        IVec3::NEG_Z,
    ];
    let attachment_faces = cab
        .keys()
        .flat_map(|cell| directions.map(|direction| *cell + direction))
        .filter(|neighbor| carved_hull.contains_key(neighbor))
        .count();
    assert!(
        attachment_faces > 0,
        "cab must be face-connected to the main hull"
    );

    assert!(
        ARROGANCE_CAB_FRONT_X < ARROGANCE_CAB_REAR_X,
        "cab nose must point out from the port bow"
    );
    assert!(
        combat_spaceship_cab_half_depth(ARROGANCE_CAB_FRONT_X)
            < combat_spaceship_cab_half_depth(ARROGANCE_CAB_REAR_X),
        "cab must widen through its full depth into the hull face"
    );
    for x in ARROGANCE_CAB_FRONT_X..=ARROGANCE_CAB_REAR_X {
        assert!(
            carrier.contains(&IVec3::new(
                x,
                combat_spaceship_cab_floor_y(x),
                ARROGANCE_CAB_CENTER_Z,
            )),
            "cab floor must remain continuous through x={x}"
        );
    }
    let door = combat_spaceship_cab_door();
    for cell in &door.cells {
        assert!(
            !carrier.contains(cell),
            "automatic door aperture is blocked at {cell:?}"
        );
    }
    let panels = voxel_auto_door_panels(&door);
    assert!(panels.iter().all(|panel| {
        panel.open_translation != panel.closed_translation
            && panel.trigger_center == door.trigger_center
    }));
    for x in ARROGANCE_CAB_REAR_X..=ARROGANCE_CAB_REAR_X + 3 {
        for y in 1..=ARROGANCE_CAB_DOOR_HEIGHT {
            assert!(
                !carrier.contains(&IVec3::new(
                    x,
                    y,
                    ARROGANCE_CAB_CENTER_Z
                )),
                "cab-to-hull doorway is blocked at {x}, {y}"
            );
        }
    }

    let original_min = original.keys().copied().reduce(IVec3::min).unwrap();
    let original_max = original.keys().copied().reduce(IVec3::max).unwrap();
    let scaled_min = scaled_original.keys().copied().reduce(IVec3::min).unwrap();
    let scaled_max = scaled_original.keys().copied().reduce(IVec3::max).unwrap();
    assert_eq!(
        scaled_min,
        scale_combat_spaceship_cell(original_min)
    );
    assert_eq!(
        scaled_max,
        scale_combat_spaceship_cell(original_max) + IVec3::splat(ARROGANCE_SCALE - 1)
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
        for cell in world_cells.iter().filter(|cell| cell.y == HANGAR_PARKING_Y) {
            assert!(
                carrier.contains(&IVec3::new(
                    cell.x,
                    HANGAR_PARKING_Y - 1,
                    cell.z
                )),
                "{name} is not parked on 狂妄号's real deck below {cell:?}"
            );
            assert!(
                carrier.contains(&IVec3::new(
                    cell.x,
                    HANGAR_CEILING_Y,
                    cell.z
                )),
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
    assert!(launched.contains::<SweptCcd>());
    let moving_layers = *launched.get::<CollisionLayers>().unwrap();
    assert_eq!(
        moving_layers,
        moving_voxel_spaceship_collision_layers()
    );
    assert!(moving_layers.interacts_with(CollisionLayers::DEFAULT));
    assert!(moving_layers.interacts_with(carrier_collision_layers()));
    assert!(moving_layers.interacts_with(moving_voxel_spaceship_collision_layers()));
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
fn players_follow_the_innermost_ship_without_needing_a_floor_contact() {
    let carrier = VoxelSpaceshipMotion {
        id: COMBAT_SPACESHIP_ID.to_owned(),
        previous: Transform::IDENTITY,
        current: Transform::from_translation(Vec3::new(10.0, 0.0, 0.0)),
        local_min: Vec3::splat(-20.0),
        local_max: Vec3::splat(20.0),
    };
    let parked_ship = VoxelSpaceshipMotion {
        id: "small-ship-01".to_owned(),
        previous: Transform::IDENTITY,
        current: Transform::from_translation(Vec3::new(10.0, 0.0, 5.0))
            .with_rotation(Quat::from_rotation_y(0.5)),
        local_min: Vec3::splat(-2.0),
        local_max: Vec3::splat(2.0),
    };
    // The player is floating inside both volumes, with no floor/collision query.
    let player = Transform::from_translation(Vec3::new(0.0, 1.5, 0.0));

    let carried =
        carried_transform_in_innermost_spaceship(&player, &[carrier, parked_ship.clone()]).unwrap();
    let expected = parked_ship
        .current
        .compute_affine()
        .transform_point3(player.translation);

    assert!(carried.translation.abs_diff_eq(expected, 0.000_01));
    assert!(carried
        .rotation
        .abs_diff_eq(parked_ship.current.rotation, 0.000_01));
}

#[test]
fn spaceship_occupant_bounds_use_canonical_voxel_extents() {
    let body = VoxelPhysicsBody {
        local_center: Vec3::ZERO,
        cells: vec![(IVec3::new(-2, 1, 4), 1), (IVec3::new(3, 5, 8), 1)],
    };

    let (min, max) = voxel_spaceship_local_bounds(&body).unwrap();

    assert_eq!(
        min,
        IVec3::new(-2, 1, 4).as_vec3() * VOXEL_SIZE
    );
    assert_eq!(
        max,
        IVec3::new(4, 6, 9).as_vec3() * VOXEL_SIZE
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

#[test]
fn spaceship_chase_camera_stays_behind_and_above_the_canonical_hull() {
    let body = VoxelPhysicsBody {
        local_center: Vec3::ZERO,
        cells: vec![(IVec3::new(-4, -1, -8), 1), (IVec3::new(4, 3, 8), 1)],
    };
    let ship_transform = Transform::from_rotation(Quat::from_rotation_y(0.7));

    let camera =
        voxel_spaceship_chase_camera_transform(&ship_transform, &body, Vec3::NEG_Z).unwrap();
    let local_camera = ship_transform
        .compute_affine()
        .inverse()
        .transform_point3(camera.translation);
    let (_, local_max) = voxel_spaceship_chase_bounds(&body).unwrap();

    assert!(local_camera.y > local_max.y);
    assert!(local_camera.z > local_max.z);
    assert!((camera.rotation * Vec3::NEG_Z).dot(ship_transform.rotation * Vec3::NEG_Z) > 0.5);
}

#[test]
fn spaceship_chase_camera_follows_a_forward_axis_other_than_local_z() {
    let body = VoxelPhysicsBody {
        local_center: Vec3::ZERO,
        cells: vec![(IVec3::new(-20, -1, -6), 1), (IVec3::new(20, 3, 6), 1)],
    };
    let ship_transform = Transform::from_rotation(Quat::from_rotation_y(0.7));

    let camera =
        voxel_spaceship_chase_camera_transform(&ship_transform, &body, Vec3::NEG_X).unwrap();
    let local_camera = ship_transform
        .compute_affine()
        .inverse()
        .transform_point3(camera.translation);
    let (_, local_max) = voxel_spaceship_chase_bounds(&body).unwrap();

    assert!(local_camera.x > local_max.x);
    assert!(local_camera.y > local_max.y);
    assert!((camera.rotation * Vec3::NEG_Z).dot(ship_transform.rotation * Vec3::NEG_X) > 0.5);
}

#[test]
fn spaceship_heading_yaw_maps_view_forward_onto_the_bow_axis() {
    for axis in [Vec3::NEG_Z, Vec3::NEG_X] {
        let view_forward = voxel_spaceship_heading_yaw(axis) * Vec3::NEG_Z;
        assert!(
            view_forward.dot(axis) > 0.999_9,
            "heading yaw for {axis:?} looks along {view_forward:?} instead of {axis:?}"
        );
    }
}

#[test]
fn default_spaceship_specs_author_forward_axes_matching_each_hull() {
    let specs = default_voxel_spaceship_specs();
    assert_eq!(
        specs.len(),
        TELEPORT_SPACESHIP_IDS.len()
    );
    for spec in &specs {
        let expected = if spec.ship.id == COMBAT_SPACESHIP_ID { Vec3::NEG_X } else { Vec3::NEG_Z };
        assert_eq!(
            spec.ship.forward_axis, expected,
            "{} bow axis must be {expected:?}",
            spec.ship.name
        );
    }
}

#[test]
fn spaceship_chase_camera_distance_scales_with_voxel_hull_size() {
    let small = VoxelPhysicsBody {
        local_center: Vec3::ZERO,
        cells: vec![(IVec3::new(-2, 0, -4), 1), (IVec3::new(2, 2, 4), 1)],
    };
    let large = VoxelPhysicsBody {
        local_center: Vec3::ZERO,
        cells: vec![(IVec3::new(-20, 0, -40), 1), (IVec3::new(20, 16, 40), 1)],
    };
    let ship_transform = Transform::IDENTITY;

    let small_camera =
        voxel_spaceship_chase_camera_transform(&ship_transform, &small, Vec3::NEG_Z).unwrap();
    let large_camera =
        voxel_spaceship_chase_camera_transform(&ship_transform, &large, Vec3::NEG_Z).unwrap();

    assert!(large_camera.translation.z > small_camera.translation.z);
    assert!(large_camera.translation.y > small_camera.translation.y);
}

#[test]
fn stopping_spaceship_control_returns_to_cockpit_view() {
    let mut control = VoxelSpaceshipControlState::default();
    control.driving_ship_id = Some("small-ship-01".to_owned());
    control.third_person_view = true;

    control.stop_driving();

    assert!(!control.third_person_view);
}

#[test]
fn spaceship_explosion_removes_only_cells_inside_the_blast_radius() {
    let transform = Transform::from_translation(Vec3::new(4.0, 2.0, -3.0)).with_rotation(
        Quat::from_rotation_y(std::f32::consts::FRAC_PI_2),
    );
    let cells = HashMap::from([
        (IVec3::ZERO, 1),
        (IVec3::X, 2),
        (IVec3::new(8, 0, 0), 3),
        (IVec3::new(0, 0, 2), 4),
    ]);
    let blast_origin = transform
        .compute_affine()
        .transform_point3(Vec3::splat(0.5) * VOXEL_SIZE);
    let mut occupancy = VoxelSpaceshipOccupancy::empty();
    for (cell, material) in &cells {
        occupancy.set_cell(*cell, *material);
    }

    // The explosion radius is clamped to one canonical voxel; the neighbour
    // sits exactly one voxel away and is included, while cells farther out
    // survive and the fluid cell is never selected as debris.
    let removed =
        voxel_spaceship_cells_in_radius(&occupancy, &transform, blast_origin, VOXEL_SIZE);
    assert_eq!(removed, vec![
        (IVec3::ZERO, 1),
        (IVec3::X, 2)
    ]);

    for &(cell, _) in &removed {
        occupancy.remove_cell(cell);
    }
    assert!(occupancy.cell_material(IVec3::new(8, 0, 0)).is_some());
    assert!(occupancy.cell_material(IVec3::new(0, 0, 2)).is_some());
    assert!(occupancy.cell_material(IVec3::ZERO).is_none());
    assert!(occupancy.cell_material(IVec3::X).is_none());

    // After the crater, a larger radius reaches the far solid survivor while
    // the fluid cell at (0,0,2) is still never selected as debris.
    let local = voxel_spaceship_cells_in_radius(
        &occupancy,
        &transform,
        blast_origin,
        VOXEL_SIZE * 8.5,
    );
    assert_eq!(
        local.iter().map(|(cell, _)| *cell).collect::<HashSet<_>>(),
        HashSet::from([IVec3::new(8, 0, 0)])
    );
}

#[test]
fn spaceship_local_edit_rebuilds_hull_and_keeps_the_ship_identity() {
    #[derive(Resource, Default)]
    struct TestShipHandle(Option<Entity>);

    fn spawn_ship(
        mut commands: Commands,
        mut meshes: ResMut<Assets<Mesh>>,
        materials: Res<VoxelMaterials>,
        mut handle: ResMut<TestShipHandle>,
        mut spawned: Local<bool>,
    ) {
        if *spawned {
            return;
        }
        *spawned = true;
        let spec = default_voxel_spaceship_specs().remove(1);
        handle.0 = Some(spawn_voxel_spaceship(
            &mut commands,
            &mut meshes,
            &materials,
            &spec,
            Transform::IDENTITY,
            LinearVelocity::ZERO,
            AngularVelocity::ZERO,
            None,
        ));
    }

    fn edit_ship(
        mut commands: Commands,
        mut occupancy: ResMut<VoxelSpaceshipOccupancyCache>,
        handle: ResMut<TestShipHandle>,
        mut applied: Local<bool>,
        ships: Query<&VoxelPhysicsBody>,
    ) {
        if *applied {
            return;
        }
        let Some(ship) = handle.0 else {
            return;
        };
        let Ok(body) = ships.get(ship) else {
            return;
        };
        let removed = body
            .cells
            .iter()
            .copied()
            .find(|(_, material)| TrpgVoxelConnector::solid(material))
            .unwrap();
        let entry = occupancy
            .ships
            .entry(ship)
            .or_insert_with(VoxelSpaceshipOccupancy::empty);
        for &(cell, material) in &body.cells {
            entry.set_cell(cell, material);
        }
        entry.remove_cell(removed.0);
        entry.cells_dirty = true;
        commands.entity(ship).insert(VoxelSpaceshipNeedsRebuild);
        *applied = true;
    }

    let mut app = App::new();
    app.init_resource::<Assets<Mesh>>()
        .insert_resource(VoxelMaterials {
            handles: std::array::from_fn(|_| Handle::default()),
            planet_ocean: Handle::default(),
        })
        .init_resource::<TestShipHandle>()
        .init_resource::<VoxelSpaceshipOccupancyCache>()
        .init_resource::<Time>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_systems(
            Update,
            (
                spawn_ship,
                edit_ship,
                rebuild_dirty_voxel_spaceships,
            )
                .chain(),
        );
    app.update();
    app.update();

    let ship = app.world().resource::<TestShipHandle>().0.unwrap();
    let original_len = default_voxel_spaceship_specs()[1].cells.len();
    {
        let entity = app.world().entity(ship);
        assert_eq!(
            entity.get::<VoxelPhysicsBody>().unwrap().cells.len(),
            original_len - 1
        );
        assert!(
            entity.contains::<VoxelSpaceship>(),
            "a locally damaged ship must keep its teleport/drive identity"
        );
        assert!(
            entity.get::<VoxelSpaceshipChunks>().unwrap().0.len() > 0,
            "the ship must keep its hull chunk registry"
        );
    }
    let mut chunk_colliders = app
        .world_mut()
        .query_filtered::<&Collider, With<VoxelSpaceshipChunk>>();
    assert!(
        chunk_colliders
            .iter(app.world())
            .all(|collider| collider.shape().as_voxels().is_some()),
        "edited chunk colliders must stay canonical voxel colliders"
    );
    let mut hull_children = app
        .world_mut()
        .query_filtered::<Entity, With<VoxelSpaceshipHullMesh>>();
    let rebuilt_hull_count = hull_children.iter(app.world()).count();
    assert!(
        rebuilt_hull_count > 0,
        "the edited chunk must respawn hull meshes from the surviving voxels"
    );
}

#[test]
fn spaceship_hull_chunks_rebuild_during_a_stroke_and_body_syncs_on_release() {
    #[derive(Resource, Default)]
    struct TestShipHandle(Option<Entity>);

    fn spawn_ship(
        mut commands: Commands,
        mut meshes: ResMut<Assets<Mesh>>,
        materials: Res<VoxelMaterials>,
        mut handle: ResMut<TestShipHandle>,
        mut spawned: Local<bool>,
    ) {
        if *spawned {
            return;
        }
        *spawned = true;
        let spec = default_voxel_spaceship_specs().remove(1);
        handle.0 = Some(spawn_voxel_spaceship(
            &mut commands,
            &mut meshes,
            &materials,
            &spec,
            Transform::IDENTITY,
            LinearVelocity::ZERO,
            AngularVelocity::ZERO,
            None,
        ));
    }

    fn edit_ship(
        mut commands: Commands,
        mut occupancy: ResMut<VoxelSpaceshipOccupancyCache>,
        handle: ResMut<TestShipHandle>,
        mut applied: Local<bool>,
        ships: Query<&VoxelPhysicsBody>,
    ) {
        if *applied {
            return;
        }
        let Some(ship) = handle.0 else {
            return;
        };
        let Ok(body) = ships.get(ship) else {
            return;
        };
        let removed = body
            .cells
            .iter()
            .copied()
            .find(|(_, material)| TrpgVoxelConnector::solid(material))
            .unwrap();
        let entry = occupancy
            .ships
            .entry(ship)
            .or_insert_with(VoxelSpaceshipOccupancy::empty);
        for &(cell, material) in &body.cells {
            entry.set_cell(cell, material);
        }
        entry.remove_cell(removed.0);
        entry.cells_dirty = true;
        commands.entity(ship).insert(VoxelSpaceshipNeedsRebuild);
        *applied = true;
    }

    let mut app = App::new();
    app.init_resource::<Assets<Mesh>>()
        .insert_resource(VoxelMaterials {
            handles: std::array::from_fn(|_| Handle::default()),
            planet_ocean: Handle::default(),
        })
        .init_resource::<TestShipHandle>()
        .init_resource::<VoxelSpaceshipOccupancyCache>()
        .init_resource::<Time>()
        .init_resource::<ButtonInput<MouseButton>>()
        .add_systems(
            Update,
            (
                spawn_ship,
                edit_ship,
                rebuild_dirty_voxel_spaceships,
            )
                .chain(),
        );
    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .press(MouseButton::Left);
    app.update();

    let ship = app.world().resource::<TestShipHandle>().0.unwrap();
    let original_len = default_voxel_spaceship_specs()[1].cells.len();
    assert!(
        app.world()
            .entity(ship)
            .get::<VoxelPhysicsBody>()
            .unwrap()
            .cells
            .len()
            == original_len,
        "a held stroke must not rebuild the body collider on every edit tick"
    );
    assert!(
        !app
            .world()
            .entity(ship)
            .contains::<VoxelSpaceshipNeedsRebuild>(),
        "the touched hull chunk must rebuild immediately, not wait for release"
    );
    let mut hull_children = app
        .world_mut()
        .query_filtered::<Entity, With<VoxelSpaceshipHullMesh>>();
    assert!(
        hull_children.iter(app.world()).next().is_some(),
        "the edited chunk must still own rebuilt hull meshes"
    );

    app.world_mut()
        .resource_mut::<ButtonInput<MouseButton>>()
        .release(MouseButton::Left);
    app.update();
    assert_eq!(
        app.world()
            .entity(ship)
            .get::<VoxelPhysicsBody>()
            .unwrap()
            .cells
            .len(),
        original_len - 1,
        "releasing the stroke must re-sync the physics body cells once"
    );
}

#[test]
fn spawned_spaceships_track_hull_surface_children_and_micro_tiles() {
    #[derive(Resource, Default)]
    struct TestShipHandle(Option<Entity>);

    fn spawn_ship(
        mut commands: Commands,
        mut meshes: ResMut<Assets<Mesh>>,
        materials: Res<VoxelMaterials>,
        mut handle: ResMut<TestShipHandle>,
        mut spawned: Local<bool>,
    ) {
        if *spawned {
            return;
        }
        *spawned = true;
        let spec = default_voxel_spaceship_specs().remove(0);
        handle.0 = Some(spawn_voxel_spaceship(
            &mut commands,
            &mut meshes,
            &materials,
            &spec,
            Transform::IDENTITY,
            LinearVelocity::ZERO,
            AngularVelocity::ZERO,
            None,
        ));
    }

    let mut app = App::new();
    app.init_resource::<Assets<Mesh>>()
        .insert_resource(VoxelMaterials {
            handles: std::array::from_fn(|_| Handle::default()),
            planet_ocean: Handle::default(),
        })
        .init_resource::<TestShipHandle>()
        .add_systems(Update, spawn_ship);
    app.update();

    let ship = app.world().resource::<TestShipHandle>().0.unwrap();
    let entity = app.world().entity(ship);
    assert!(entity.contains::<VoxelSpaceshipMicroTiles>());
    assert!(
        entity.get::<VoxelSpaceshipChunks>().unwrap().0.len() > 0,
        "a spawned ship must partition its hull into chunks"
    );
    let mut hull_children = app
        .world_mut()
        .query_filtered::<Entity, With<VoxelSpaceshipHullMesh>>();
    let mut micro_children = app
        .world_mut()
        .query_filtered::<Entity, With<VoxelMicroDecoration>>();
    let hull_count = hull_children.iter(app.world()).count();
    let micro_count = micro_children.iter(app.world()).count();
    assert!(hull_count > 0);
    assert!(micro_count > 0);
}

#[test]
fn spawned_hull_chunks_never_carry_empty_voxel_colliders() {
    fn spawn_fleet(
        mut commands: Commands,
        mut meshes: ResMut<Assets<Mesh>>,
        materials: Res<VoxelMaterials>,
    ) {
        for spec in default_voxel_spaceship_specs() {
            spawn_voxel_spaceship(
                &mut commands,
                &mut meshes,
                &materials,
                &spec,
                spec.transform,
                LinearVelocity::ZERO,
                AngularVelocity::ZERO,
                spec.docking.clone(),
            );
        }
    }

    let mut app = App::new();
    app.init_resource::<Assets<Mesh>>()
        .insert_resource(VoxelMaterials {
            handles: std::array::from_fn(|_| Handle::default()),
            planet_ocean: Handle::default(),
        })
        .add_systems(Update, spawn_fleet);
    app.update();

    let mut chunks = app
        .world_mut()
        .query_filtered::<(Entity, Option<&Collider>), With<VoxelSpaceshipChunk>>();
    let mut solid_chunk_count = 0;
    for (_, collider) in chunks.iter(app.world()) {
        let Some(collider) = collider else {
            continue;
        };
        solid_chunk_count += 1;
        let voxels = collider
            .shape()
            .as_voxels()
            .expect("chunk colliders must stay voxel colliders");
        assert!(
            voxels.voxels().next().is_some(),
            "a chunk collider must never be empty: Avian panics on empty voxel AABBs"
        );
    }
    assert!(
        solid_chunk_count > 0,
        "the fleet must own solid hull chunks"
    );
}
