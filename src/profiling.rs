use std::collections::HashMap;

use avian3d::prelude::{
    ContactGraph,
    RigidBody,
    Sleeping,
};
use bevy::prelude::*;

/// Summarize collision workload once, after startup, in profiling builds only.
pub(crate) fn report_physics_workload(
    time: Res<Time<Real>>,
    graph: Option<Res<ContactGraph>>,
    bodies: Query<(Entity, &RigidBody, Has<Sleeping>)>,
    mut reported: Local<bool>,
) {
    if *reported || time.elapsed_secs() < 25.0 {
        return;
    }
    *reported = true;
    let Some(graph) = graph else {
        return;
    };
    let mut pairs = HashMap::new();
    for pair in graph.iter_active() {
        *pairs.entry((pair.body1, pair.body2)).or_insert(0usize) += 1;
    }
    let mut pairs = pairs.into_iter().collect::<Vec<_>>();
    pairs.sort_by_key(|(_, count)| *count);
    info!(
        active_pairs = graph.iter_active().count(),
        sleeping_pairs = graph.iter_sleeping().count(),
        "Physics profile"
    );
    let dynamic_bodies = bodies
        .iter()
        .filter(|(_, body, ..)| **body == RigidBody::Dynamic)
        .count();
    let sleeping_bodies = bodies.iter().filter(|(_, _, sleeping)| *sleeping).count();
    info!(
        dynamic_bodies,
        sleeping_bodies, "Physics body counts"
    );
    for (pair, count) in pairs.into_iter().rev().take(20) {
        info!(?pair, count, "Physics pair profile");
    }
}
