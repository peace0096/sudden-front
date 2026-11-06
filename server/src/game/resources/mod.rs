//! ECS resources: one match's shared state, one domain per file.

mod arena;
mod event_log;
mod match_state;
mod projectile;
mod rng;
mod rules;

pub use arena::*;
pub use event_log::*;
pub use match_state::*;
pub use projectile::*;
pub use rng::*;
pub use rules::*;
