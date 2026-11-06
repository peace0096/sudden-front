//! Server-authoritative game rules, run in order every tick. Each file holds one domain's system
//! and the rules it applies; this module wires them into the tick's schedule.

pub(super) mod blast;
pub(super) mod bot;
pub(super) mod fire;
pub(super) mod grenade;
pub(super) mod history;
pub(super) mod input;
pub(super) mod king;
pub(super) mod missile;
pub(super) mod perk;
pub(super) mod respawn;
pub(super) mod tick;
pub(super) mod weapon;

use bevy_ecs::prelude::*;
use bevy_ecs::schedule::ExecutorKind;

/// A match's tick: every system once, in this order, on one thread.
pub fn schedule() -> Schedule {
    let mut schedule = Schedule::default();
    schedule.set_executor_kind(ExecutorKind::SingleThreaded);
    schedule.add_systems(
        (
            bot::bot_think,
            input::consume_inputs,
            weapon::update_weapons,
            fire::fire_weapons,
            perk::update_perks,
            king::update_kings,
            respawn::respawn_players,
            history::record_history,
            tick::advance_tick,
        )
            .chain(),
    );
    schedule
}
