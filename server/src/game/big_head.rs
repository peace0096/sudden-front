//! Big Head mode: a level that kills raise and deaths lower, with a head (model and hitbox) to
//! match, and the King's reign at the top level.

use super::components::{Perks, Vitals, Weapon};
use super::TICK_RATE;

/// The lowest level, which dying again and again comes down to, and the highest, which crowns the
/// King.
pub const MIN_LEVEL: i32 = -5;
pub const MAX_LEVEL: i32 = 3;
/// Where a kill takes a player whose level is below 0: straight up, past 0.
pub const COMEBACK_LEVEL: i32 = 1;
/// Where a reign leaves the King: the normal head, to climb again.
pub const KING_END_LEVEL: i32 = 0;
/// Seconds a reign lasts: invulnerable, a little faster (the hammer's move speed) and with nothing
/// but the hammer. Counted in whole ticks, so it is exact.
pub const KING_TIME: f32 = 15.0;
pub const KING_TICKS: u32 = (KING_TIME * TICK_RATE as f32) as u32;
/// Head size, of the model and of its hitbox, at each level from MIN_LEVEL to MAX_LEVEL: 1 at level 0,
/// a dot at the bottom, the King's at the top. Within augments.rs BIG_HEAD_MAX, which bounds the
/// character model (client/src/game/art/hitbox.ts HEAD_SCALE_MAX).
const HEAD_SCALES: [f32; (MAX_LEVEL - MIN_LEVEL + 1) as usize] = [0.4, 0.5, 0.6, 0.7, 0.85, 1.0, 2.0, 3.5, 5.0];

/// The head's size at `level`, clamped into the levels there are.
pub fn head_scale(level: i32) -> f32 {
    HEAD_SCALES[(level.clamp(MIN_LEVEL, MAX_LEVEL) - MIN_LEVEL) as usize]
}

/// The level a kill takes a player to: up one to MAX_LEVEL, or from below 0 to COMEBACK_LEVEL.
pub fn after_kill(level: i32) -> i32 {
    if level < 0 { COMEBACK_LEVEL } else { (level + 1).min(MAX_LEVEL) }
}

/// The level a death takes a player to: down one, to MIN_LEVEL.
pub fn after_death(level: i32) -> i32 {
    (level - 1).max(MIN_LEVEL)
}

/// Crowns a living player at MAX_LEVEL: the reign's clock starts, health is full, a paralysis ends
/// and the hammer replaces every other weapon until the reign is over.
pub fn crown(perks: &mut Perks, vitals: &mut Vitals, weapon: &mut Weapon, max_hp: i32) {
    perks.king_ticks = KING_TICKS;
    vitals.hp = max_hp;
    vitals.stun_left = 0.0;
    weapon.take_hammer();
}
