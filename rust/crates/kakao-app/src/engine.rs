// Pure-move split of the former monolithic `engine.rs` (SRP).
//
// - `model`: snapshot/stale types + restore cadence constants
// - `caches`: worker-owned `EngineCaches`
// - `flags`: cross-thread `SharedFlags`
// - `apply`: `apply_evaluation` (Win32 mutation) + snapshot capture
// - `restore`: restore-all/stale-restore with backoff + `report_restore`
// - `schedule`: PID-scan / reconciliation cadence (pure, unit-tested)
// - `tick`: single reconciliation step
// - `worker`: background `spawn_worker` loop
//
// No reconcile/restore semantics changed; this file only re-exports the
// previous public surface.
mod apply;
mod caches;
mod flags;
mod model;
mod restore;
pub mod schedule;
mod tick;
mod worker;

pub use apply::{apply_evaluation, capture_snapshot};
pub use caches::EngineCaches;
pub use flags::SharedFlags;
pub use model::{RestoreSnapshot, StaleState, RESTORE_MISS_THRESHOLD};
pub use restore::restore_all;
pub use tick::tick;
pub use worker::{join_with_timeout, spawn_worker};
