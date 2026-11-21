//! ground finds the floor under a point with one trace straight down, the
//! way a mode drops a hero, a marker or an effect onto the map:
//!
//! ```ignore
//! let floor = ground(spawn, Ground::default()).unwrap_or(spawn);
//! let deep = ground(point, Ground { above: 300.0, below: 900.0, layers: Layers::SOLID });
//! ```

use crate::bits::Layers;
use crate::generated::{trace, TraceOptions, Vector};

/// Ground shapes the trace ground runs; its default suits a map's floor.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Ground {
    /// above is how far over the point the trace starts, 64 units by default.
    pub above: f64,
    /// below is how far under the point the trace ends, 1024 units by
    /// default.
    pub below: f64,
    /// layers is what counts as floor: solid and player clip by default.
    pub layers: u64,
}

impl Default for Ground {
    fn default() -> Self {
        Self {
            above: 64.0,
            below: 1024.0,
            layers: Layers::SOLID | Layers::PLAYER_CLIP,
        }
    }
}

/// ground returns the floor under point, searched from just above it. It
/// returns None when no floor answers in reach, when the trace starts inside
/// a wall, and before the world's collision is ready.
pub fn ground(point: Vector, options: Ground) -> Option<Vector> {
    let hit = trace(&TraceOptions {
        start: Vector {
            z: point.z + options.above,
            ..point
        },
        end: Vector {
            z: point.z - options.below,
            ..point
        },
        layers: Some(options.layers),
        ..TraceOptions::default()
    })?;
    (!hit.start_solid).then_some(hit.position)
}
