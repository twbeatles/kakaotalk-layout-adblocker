// Pure-move split of the former monolithic `evaluate.rs` (SRP).
//
// - `payloads`: diagnostic DTOs (`Evaluation`, `ActionLog`, ...)
// - `mutation_log`: per-tick mutation accumulator
// - `inspect`: read-only candidate/main-window inspection
// - `apply`: mutation planning from inspection results
// - `orchestrate`: `evaluate_graph*` entry points
//
// No ranking/filtering semantics changed; this file only re-exports the
// previous public surface.
mod apply;
mod inspect;
mod mutation_log;
mod orchestrate;
mod payloads;

pub use orchestrate::{
    evaluate_dump, evaluate_graph, evaluate_graph_for_apply, evaluate_graph_with_states,
};
pub use payloads::{
    ActionLog, AdSignalsPayload, CandidatePayload, EngineStatePayload, Evaluation, GoldenFile,
    MainWindowPayload,
};
