# Profiling development builds

Run `cargo r --features profiling` to enable Bevy's built-in Tracy CPU/GPU
instrumentation and additional chat UI spans. This keeps the normal development
profile; it does not require a release build. The profiling build also logs a
one-time physics workload summary after 25 seconds.

Match the capture tool to the resolved `tracy-client-sys` version. The current
lockfile uses `0.29.0`, which requires Tracy **0.14.0**. See the
[client version table](https://github.com/nagisa/rust_tracy_client#version-support-table)
and [Bevy profiling guide](https://docs.rs/crate/bevy/latest/source/docs/profiling.md).

Start a command-line capture before launching the app:

```powershell
tracy-capture -a 127.0.0.1 -o capture.tracy -s 45
```

Open the capture in Tracy, or export statistics with:

```powershell
tracy-csvexport capture.tracy > stats.csv
tracy-csvexport -u -f update capture.tracy > frames.csv
tracy-csvexport -g capture.tracy > gpu.csv
```

Compare the same scene and build profile. Exclude startup and the first 20 seconds
when measuring steady-state performance: loose voxel props and fragments become
static after **15 seconds** of simulation. They keep their geometry and collision
shape, and dragging or applying a physics-tool impulse reactivates them and resets
the timer. Ships are excluded. Chunk streaming and in-memory scene snapshots retain
the timer and static state. On application startup, all saved loose voxel bodies
load as static with zero velocity, including bodies in older saves. They do not
restart the 15-second timer. No existing binary scene format is changed.

The initial September 27 capture measured 215.3 ms mean steady-state CPU frame
time (about 4.6 FPS). The fixed physics loop consumed 168.3 ms/frame on average;
collision narrow phase alone consumed 24.5 seconds of the 45.8-second capture.
A subsequent collision inventory found 1,231 dynamic bodies and 59,613 active
collision pairs. This identified persistent loose-body simulation as the primary
CPU bottleneck.

With the 15-second cutoff and non-waking gravity, the comparison capture measured
35.9 ms mean frame time (about 27.9 FPS), 33.6 ms median, and 51.8 ms p95 after
20 seconds. Active collision pairs fell to 423, with nine dynamic bodies remaining
(ships and the player). These are development-build measurements on the same
machine and saved scene, with a separate unrelated compilation also running;
they are not a portable FPS guarantee. UI and scene rendering remain visible costs.

The traces and exported statistics from this investigation are in the ignored
`.data/profiling/` directory. The comparison above predates the additional change
to avoid writing unchanged ship trajectory history every half-second.
