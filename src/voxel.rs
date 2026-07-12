use bevy::prelude::*;
use voxxelmaxx::prelude::*;

pub struct TrpgVoxelPlugin;

pub struct TrpgVoxelConnector;

impl Connector for TrpgVoxelConnector {
    type Item = u8;

    fn solid(voxel: &Self::Item) -> bool { *voxel != 0 }
}

#[derive(Component)]
pub struct TrpgVoxelGrid;

impl Plugin for TrpgVoxelPlugin {
    fn build(&self, app: &mut App) {
        app.add_plugins((
            VoxelPlugin::<u8>::default(),
            ConnectivityPlugin::<TrpgVoxelConnector>::default(),
        ))
        .add_systems(Startup, setup_voxel_grid);
    }
}

fn setup_voxel_grid(mut commands: Commands) {
    commands.spawn((
        TrpgVoxelGrid,
        Grid::<u8>::new(),
        BodyTracker::<TrpgVoxelConnector>::new(),
        Boundary,
    ));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn plugin_initializes_one_empty_trpg_grid() {
        let mut app = App::new();
        app.add_plugins(TrpgVoxelPlugin);
        app.update();

        let world = app.world_mut();
        let mut grids = world.query_filtered::<&Grid<u8>, With<TrpgVoxelGrid>>();
        let grid = grids.single(world).unwrap();
        assert_eq!(grid.count(), 0);
    }

    #[test]
    fn connector_treats_zero_as_air() {
        assert!(!TrpgVoxelConnector::solid(&0));
        assert!(TrpgVoxelConnector::solid(&1));
        assert!(TrpgVoxelConnector::solid(&u8::MAX));
    }
}
