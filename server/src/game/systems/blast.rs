//! Blasts: a missile or grenade going off hurts every enemy it reaches in the open.

use super::fire::{ray_shield, Hit, TargetView};
use crate::game::physics::line_clear;
use crate::game::resources::*;
use crate::game::*;
use crate::pb;
use bevy_ecs::prelude::*;

/// Share of a blast's damage lost by the edge of its radius.
const SPLASH_FALLOFF: f32 = 0.7;

/// Who set off a blast (fired the missile, threw the grenade), and with what.
#[derive(Clone, Copy)]
pub(super) struct Blaster {
    pub(super) owner: Entity,
    pub(super) owner_id: u32,
    pub(super) team: pb::Team,
    pub(super) weapon: pb::WeaponKind,
}

/// Whether a missile in flight, or a blast, can touch or hurt `target`. Never its owner: a missile
/// starts out inside their hit range, nobody is hurt by their own blast, and in a free-for-all
/// their team is no protection.
pub(super) fn blast_target(rules: &Rules, source: &Blaster, target: &TargetView) -> bool {
    target.hittable && rules.hostile(source.team, target.team) && target.entity != source.owner
}

/// Damages every enemy of the owner the blast at `center` reaches in the open: the weapon's damage
/// at the centre, falling off with distance to its splash radius.
pub(super) fn blast(
    boxes: &[pb::MapBox],
    rules: &Rules,
    center: [f32; 3],
    source: &Blaster,
    targets: &[TargetView],
    hits: &mut Vec<Hit>,
) {
    let stats = weapons::stats(source.weapon);
    for target in targets.iter().filter(|s| blast_target(rules, source, s)) {
        let (x, feet, z, crouching, yaw) = (
            target.pose.x,
            target.pose.y,
            target.pose.z,
            target.pose.crouching,
            target.pose.yaw,
        );
        let torso = [x, feet + if crouching { 0.7 } else { 1.2 }, z];
        let d = [
            torso[0] - center[0],
            torso[1] - center[1],
            torso[2] - center[2],
        ];
        let distance = (d[0] * d[0] + d[1] * d[1] + d[2] * d[2]).sqrt();
        if distance >= stats.splash_radius || !line_clear(boxes, center, torso) {
            continue;
        }
        let falloff = 1.0 - SPLASH_FALLOFF * distance / stats.splash_radius;
        let to_torso = [
            d[0] / distance.max(1e-4),
            d[1] / distance.max(1e-4),
            d[2] / distance.max(1e-4),
        ];
        let shield = target.shielded
            && ray_shield(center, to_torso, x, feet, z, yaw, crouching)
                .is_some_and(|t| t < distance);
        hits.push(Hit {
            attacker: source.owner,
            attacker_id: source.owner_id,
            attacker_team: source.team,
            weapon: source.weapon,
            victim: target.entity,
            zone: pb::HitZone::Body,
            amount: (stats.damage_body as f32 * falloff) as i32,
            shield,
            stun: 0.0,
        });
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::test_support::*;

    #[test]
    fn a_missile_blast_catches_people_beside_it_but_not_behind_walls() {
        let yaw = std::f32::consts::PI;
        let dealt = |z: f32| {
            let mut game = with_augment(Aug::Missile);
            place(&mut game, SHOOTER, -31.0, 4.0, yaw);
            place(&mut game, VICTIM, -29.0, z, 0.0);
            game.tick();
            game.push_inputs(
                SHOOTER,
                vec![pb::InputCmd {
                    fire: true,
                    yaw,
                    ..cmd(1, tick_now(&game))
                }],
            );
            for _ in 0..30 {
                game.tick();
            }
            150 - hp(&game, VICTIM)
        };
        assert!(dealt(5.0) > 0);
        assert_eq!(dealt(8.5), 0, "behind the wall");
    }

    #[test]
    fn a_grenade_hurts_enemies_around_it_but_not_its_thrower_or_teammates() {
        let mut game = Match::new(
            &[
                (SHOOTER, "a".into(), pb::Team::Red, RIFLE),
                (VICTIM, "b".into(), pb::Team::Blue, RIFLE),
                (MATE, "c".into(), pb::Team::Red, RIFLE),
            ],
            YARD,
            false,
            42,
        );
        place(&mut game, SHOOTER, -44.0, 0.0, 0.0);
        place(&mut game, VICTIM, -42.5, 0.0, 0.0);
        place(&mut game, MATE, -44.0, 1.0, 0.0);
        drop_grenade(&mut game, SHOOTER, -44.0, 0.5);
        assert!(kills_in_two_ticks(&mut game).is_empty());
        // 1.5 m off along the floor, 1.14 m below the torso: 1.88 m from it, 74 % of the damage.
        let dealt = 100 - hp(&game, VICTIM);
        assert!((75..=85).contains(&dealt), "dealt {dealt}");
        assert_eq!(hp(&game, SHOOTER), 100, "no damage to yourself");
        assert_eq!(hp(&game, MATE), 100, "no friendly fire");

        // Further out it hurts less, and past its radius not at all.
        let at = |distance: f32| {
            let mut game = duel();
            place(&mut game, SHOOTER, -44.0, 10.0, 0.0);
            place(&mut game, VICTIM, -44.0, 0.0, 0.0);
            drop_grenade(&mut game, SHOOTER, -44.0 + distance, 0.0);
            kills_in_two_ticks(&mut game);
            100 - hp(&game, VICTIM)
        };
        assert!(at(1.0) > at(3.0) && at(3.0) > 0, "{} {}", at(1.0), at(3.0));
        assert_eq!(at(5.5), 0);

        // A blast that finishes someone off is the grenade's kill.
        game.world
            .get_mut::<Vitals>(game.entities[&VICTIM])
            .unwrap()
            .hp = 10;
        drop_grenade(&mut game, SHOOTER, -42.5, 1.0);
        let kills = kills_in_two_ticks(&mut game);
        assert_eq!(kills.len(), 1);
        assert_eq!(
            (kills[0].killer_id, kills[0].victim_id, kills[0].weapon),
            (SHOOTER, VICTIM, Kind::Grenade as i32)
        );
        assert_eq!(game.shared_snapshot().red_score, 1);
    }

    #[test]
    fn walls_shelter_from_a_grenade() {
        // The walk-through warehouse's north wall (z 7 to 7.5) between the grenade and the victim.
        let dealt = |victim_z: f32| {
            let mut game = duel();
            place(&mut game, SHOOTER, -46.0, 0.0, 0.0);
            place(&mut game, VICTIM, -30.0, victim_z, 0.0);
            drop_grenade(&mut game, SHOOTER, -30.0, 6.0);
            kills_in_two_ticks(&mut game);
            100 - hp(&game, VICTIM)
        };
        assert!(dealt(4.5) > 0);
        // Inside, between the wall and the container along it.
        assert_eq!(dealt(8.0), 0, "behind the wall");
    }
}
