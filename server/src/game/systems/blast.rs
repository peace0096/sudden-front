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
