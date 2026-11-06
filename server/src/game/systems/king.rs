//! Big Head mode: each King's reign runs down a tick at a time.

use super::weapon::issue_weapons;
use crate::game::components::*;
use crate::game::resources::*;
use crate::game::*;
use bevy_ecs::prelude::*;

/// Big Head mode: runs each King's reign down a tick. When it is over the player is back at
/// big_head::KING_END_LEVEL, the hammer gone, with a fresh set of their own weapons (full
/// ammunition, the primary in hand, as at a respawn).
pub fn update_kings(
    rules: Res<Rules>,
    mut players: Query<(&Player, &mut Perks, &mut Weapon, &Loadout)>,
) {
    if !rules.big_head {
        return;
    }
    for (player, mut perks, mut weapon, loadout) in &mut players {
        if !perks.king() {
            continue;
        }
        perks.king_ticks -= 1;
        if perks.king() {
            continue;
        }
        perks.set_level(big_head::KING_END_LEVEL);
        *weapon = issue_weapons(loadout.0, &perks.active, player.is_bot);
    }
}
