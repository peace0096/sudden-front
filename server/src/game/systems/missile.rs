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
