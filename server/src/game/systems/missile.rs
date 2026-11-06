//! Missiles: real projectiles that fly each tick and blow up on whatever they touch.

use super::blast::{blast, blast_target, Blaster};
use super::fire::{Hit, TargetView};
use crate::game::physics::{ray_aabb, ray_world};
use crate::game::resources::*;
use crate::game::*;
use crate::pb;
use crate::pb::game_event::Event;

pub const MISSILE_SPEED: f32 = 60.0;

pub(super) const MISSILE_LIFE: f32 = 3.0;

/// How far from a player's body a missile still counts as hitting them.
const MISSILE_HIT_PAD: f32 = 0.8;

/// Flies each missile one tick; whatever it touches sets it off.
pub(super) fn fly_missiles(
    boxes: &[pb::MapBox],
    rules: &Rules,
    missiles: &mut Missiles,
    targets: &[TargetView],
    log: &mut EventLog,
    hits: &mut Vec<Hit>,
) {
    missiles.live.retain_mut(|missile| {
        let (pos, dir) = (missile.pos, missile.dir);
        let source = Blaster {
            owner: missile.owner,
            owner_id: missile.owner_id,
            team: missile.team,
            weapon: pb::WeaponKind::Missile,
        };
        let step = MISSILE_SPEED * TICK_DT;
        let mut t = step;
        let mut contact = false;
        if let Some(wall) = ray_world(boxes, pos, dir, step) {
            t = wall;
            contact = true;
        }
        if dir[1] < 0.0 && pos[1] / -dir[1] < t {
            t = pos[1] / -dir[1];
            contact = true;
        }
        for target in targets.iter().filter(|s| blast_target(rules, &source, s)) {
            let (x, feet, z) = (target.pose.x, target.pose.y, target.pose.z);
            let r = 0.28 + MISSILE_HIT_PAD;
            let top = feet + PLAYER_HEIGHT + MISSILE_HIT_PAD;
            if let Some(d) = ray_aabb(pos, dir, [x - r, feet, z - r], [x + r, top, z + r]) {
                if d < t {
                    t = d;
                    contact = true;
                }
            }
        }
        // Just short of the surface, so the blast isn't on its far side.
        let t = if contact { (t - 0.05).max(0.0) } else { t };
        missile.pos = [
            pos[0] + dir[0] * t,
            pos[1] + dir[1] * t,
            pos[2] + dir[2] * t,
        ];
        missile.life -= TICK_DT;
        if contact {
            let [x, y, z] = missile.pos;
            log.push(Event::Explosion(pb::ExplosionEvent { x, y, z }));
            blast(boxes, rules, missile.pos, &source, targets, hits);
            return false;
        }
        missile.life > 0.0
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::*;

    #[test]
    fn missiles_fly_then_blast_without_hurting_the_shooter() {
        let mut game = with_augment(Aug::Missile);
        assert_eq!(held(&game, SHOOTER), Kind::Missile);
        assert_eq!(weapon(&game, SHOOTER), (9, 36, false));

        let events = shoot_once(&mut game, 1);
        assert_eq!(weapon(&game, SHOOTER).0, 8);
        assert!(events
            .iter()
            .any(|e| matches!(&e.event, Some(pb::game_event::Event::Shot(s)) if s.explosive)));
        assert_eq!(hp(&game, VICTIM), 150, "still in the air");
        assert_eq!(game.shared_snapshot().missiles.len(), 1);

        let mut exploded = false;
        for _ in 0..4 {
            game.tick();
            exploded |= game
                .shared_snapshot()
                .events
                .iter()
                .any(|e| matches!(e.event, Some(pb::game_event::Event::Explosion(_))));
        }
        assert!(exploded);
        let dealt = 150 - hp(&game, VICTIM);
        assert!(dealt > 60 && dealt <= 150, "dealt {dealt}");
        assert_eq!(hp(&game, SHOOTER), 150, "no damage to yourself");
        assert!(game.shared_snapshot().missiles.is_empty());
    }

    #[test]
    fn a_missile_has_a_wide_hit_range() {
        // Aimed 0.9 m wide of the body at 4 m: a bullet misses, a missile doesn't.
        let fire_wide = |augment: Aug| {
            let mut game = with_augment(augment);
            let mut shot = line_up(&mut game, -0.05);
            shot.yaw += (0.9f32 / 4.0).atan();
            game.push_inputs(SHOOTER, vec![shot]);
            for _ in 0..6 {
                game.tick();
            }
            150 - hp(&game, VICTIM)
        };
        assert!(fire_wide(Aug::Missile) > 0);
        assert_eq!(fire_wide(Aug::SoulDrain), 0);
    }

    #[test]
    fn a_free_for_all_missile_spares_its_shooter_and_hits_whoever_it_meets() {
        let mut game = ffa_missiles();
        let events = shoot_once(&mut game, 1);
        assert!(events
            .iter()
            .any(|e| matches!(&e.event, Some(pb::game_event::Event::Shot(s)) if s.explosive)));
        assert!(
            !events
                .iter()
                .any(|e| matches!(e.event, Some(pb::game_event::Event::Explosion(_)))),
            "it went off in the shooter's hands"
        );
        assert_eq!(game.shared_snapshot().missiles.len(), 1);

        let mut blew = false;
        for _ in 0..4 {
            game.tick();
            blew |= exploded(&mut game);
        }
        assert!(blew);
        let dealt = 150 - hp(&game, VICTIM);
        assert!(dealt > 60 && dealt <= 150, "dealt {dealt}");
        assert_eq!(hp(&game, SHOOTER), 150, "no damage to yourself");
    }

    #[test]
    fn a_kill_by_a_missile_whose_shooter_has_left_counts_for_nobody() {
        let mut game = ffa_missiles();
        // One kill would win, and the victim dies of anything.
        game.set_kills_to_win(1);
        game.world
            .get_mut::<Vitals>(game.entities[&VICTIM])
            .unwrap()
            .hp = 1;
        shoot_once(&mut game, 1);
        assert_eq!(game.shared_snapshot().missiles.len(), 1, "still in the air");
        game.remove_player(SHOOTER);
        assert_eq!(game.winner(), None, "two are left");

        let mut blew = false;
        for _ in 0..4 {
            game.tick();
            blew |= exploded(&mut game);
        }
        assert!(blew);
        assert_eq!(
            game.world
                .get::<Stats>(game.entities[&VICTIM])
                .unwrap()
                .deaths,
            1
        );
        let snapshot = game.shared_snapshot();
        assert!(snapshot.players.iter().all(|p| p.kills == 0));
        assert_eq!((snapshot.red_score, snapshot.blue_score), (0, 0));
        assert_eq!(game.winner(), None);
    }
}
