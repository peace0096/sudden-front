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
const HEAD_SCALES: [f32; (MAX_LEVEL - MIN_LEVEL + 1) as usize] =
    [0.4, 0.5, 0.6, 0.7, 0.85, 1.0, 2.0, 3.5, 5.0];

/// The head's size at `level`, clamped into the levels there are.
pub fn head_scale(level: i32) -> f32 {
    HEAD_SCALES[(level.clamp(MIN_LEVEL, MAX_LEVEL) - MIN_LEVEL) as usize]
}

/// The level a kill takes a player to: up one to MAX_LEVEL, or from below 0 to COMEBACK_LEVEL.
pub fn after_kill(level: i32) -> i32 {
    if level < 0 {
        COMEBACK_LEVEL
    } else {
        (level + 1).min(MAX_LEVEL)
    }
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

#[cfg(test)]
mod tests {
    use crate::game::test_support::*;

    #[test]
    fn big_head_levels_run_from_minus_five_to_three_with_a_head_for_each() {
        use big_head::{after_death, after_kill, head_scale, MAX_LEVEL, MIN_LEVEL};
        assert_eq!((MIN_LEVEL, MAX_LEVEL), (-5, 3));
        // Up one a kill, and from below 0 straight to 1; down one a death, to the bottom.
        assert_eq!([-5, -1, 0, 1, 2, 3].map(after_kill), [1, 1, 1, 2, 3, 3]);
        assert_eq!([3, 1, 0, -4, -5].map(after_death), [2, 0, -1, -5, -5]);
        // Every level up has a bigger head: a dot at the bottom, the normal head at 0, the King's at
        // the top, which the character model's bounds still hold.
        let heads: Vec<f32> = (MIN_LEVEL..=MAX_LEVEL).map(head_scale).collect();
        assert!(heads.windows(2).all(|pair| pair[1] > pair[0]), "{heads:?}");
        assert_eq!(head_scale(0), 1.0);
        assert!(head_scale(MIN_LEVEL) <= 0.5, "{heads:?}");
        assert!(
            head_scale(MAX_LEVEL) >= 2.0 && head_scale(MAX_LEVEL) <= augments::BIG_HEAD_MAX,
            "{heads:?}"
        );
    }

    #[test]
    fn a_big_head_kill_raises_the_killer_and_lowers_the_victim_heads_and_all() {
        let mut game = big_head_duel();
        let kills = headshot(&mut game);
        assert_eq!(kills.len(), 1);
        // A normal head is nobody's dot.
        assert_eq!(kills[0].special, pb::SpecialKill::Unspecified as i32);
        let snapshot = game.shared_snapshot();
        let state = |id: u32| snapshot.players.iter().find(|p| p.id == id).unwrap();
        assert_eq!(
            (state(SHOOTER).level, state(SHOOTER).head_scale),
            (1, big_head::head_scale(1))
        );
        assert_eq!(
            (state(VICTIM).level, state(VICTIM).head_scale),
            (-1, big_head::head_scale(-1))
        );
        assert!(big_head::head_scale(1) > 1.0 && big_head::head_scale(-1) < 1.0);
        assert_eq!(state(SHOOTER).king_left, 0.0);
    }

    #[test]
    fn a_kill_below_level_zero_goes_straight_to_level_one() {
        let mut game = big_head_duel();
        set_level(&mut game, SHOOTER, -4);
        assert_eq!(headshot(&mut game).len(), 1);
        assert_eq!(perks_of(&game, SHOOTER).level, 1);
    }

    #[test]
    fn other_modes_keep_levels_heads_and_kills_plain() {
        let mut game = duel();
        let kills = headshot(&mut game);
        assert_eq!(kills[0].special, pb::SpecialKill::Unspecified as i32);
        for id in [SHOOTER, VICTIM] {
            assert_eq!(
                (perks_of(&game, id).level, perks_of(&game, id).head_scale),
                (0, 1.0)
            );
        }
        let config = game.start_message(SHOOTER).config.unwrap();
        assert_eq!(
            (config.min_level, config.max_level, config.king_time),
            (0, 0, 0.0)
        );

        let config = big_head_duel().start_message(SHOOTER).config.unwrap();
        assert_eq!(
            (
                config.mode,
                config.min_level,
                config.max_level,
                config.king_time,
                config.max_hp
            ),
            (pb::GameMode::BigHead as i32, -5, 3, 20.0, MAX_HP)
        );
    }

    #[test]
    fn a_headshot_kill_on_the_smallest_head_is_a_dot_shot() {
        // The victim's head is the smallest, and smaller than normal.
        let mut game = big_head_duel();
        set_level(&mut game, VICTIM, -2);
        let kills = headshot(&mut game);
        assert_eq!(kills[0].special, pb::SpecialKill::DotShot as i32);
        assert_eq!(perks_of(&game, VICTIM).level, -3);

        // Tied at the bottom still counts.
        let mut game = big_head_duel();
        set_level(&mut game, SHOOTER, -2);
        set_level(&mut game, VICTIM, -2);
        assert_eq!(
            headshot(&mut game)[0].special,
            pb::SpecialKill::DotShot as i32
        );

        // Someone with a smaller head takes it away.
        let mut game = big_head_duel();
        set_level(&mut game, SHOOTER, -3);
        set_level(&mut game, VICTIM, -2);
        assert_eq!(
            headshot(&mut game)[0].special,
            pb::SpecialKill::Unspecified as i32
        );

        // So does a kill that isn't to the head.
        let mut game = big_head_duel();
        set_level(&mut game, VICTIM, -2);
        game.world
            .get_mut::<Vitals>(game.entities[&VICTIM])
            .unwrap()
            .hp = 10;
        let shot = line_up(&mut game, -0.1);
        game.push_inputs(SHOOTER, vec![shot]);
        game.tick();
        let kills = kills_now(&mut game);
        assert!(!kills[0].headshot);
        assert_eq!(kills[0].special, pb::SpecialKill::Unspecified as i32);
    }
}
