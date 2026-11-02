//! Augment mode: the card pool and how hands are dealt.

use super::components::GameRng;
use crate::pb::{Augment, AugmentGrade};

pub const AUGMENT_MAX_HP: i32 = 150;
/// Seconds to choose a card, cut short once everyone has.
pub const AUGMENT_PICK: f32 = 15.0;
/// Seconds between the cards being settled and the fight starting.
pub const AUGMENT_WAIT: f32 = 5.0;
pub const HAND_SIZE: usize = 3;

pub const VEST_MAX_HP: i32 = 200;
pub const SOUL_DRAIN_HEAL: i32 = 50;
pub const SHIELD_HP: i32 = 1200;
/// Seconds since the last shot or hit before a player counts as out of combat.
pub const REST_DELAY: f32 = 4.0;
pub const REST_HEAL_PER_SECOND: f32 = 15.0;
pub const POSSESSION_HP: i32 = 150;
/// Added to a victim's head size per Big Head holder who helped kill them.
pub const BIG_HEAD_STEP: f32 = 0.1;

const SILVER: [Augment; 6] = [
    Augment::InfiniteAmmo,
    Augment::SoulDrain,
    Augment::PistolUpgrade,
    Augment::QuickReload,
    Augment::Vest,
    Augment::Taser,
];
const GOLD: [Augment; 8] = [
    Augment::Shotgun,
    Augment::Rest,
    Augment::Shield,
    Augment::SmallHead,
    Augment::Possession,
    Augment::BigHead,
    Augment::Magnum,
    Augment::DoubleTime,
];
// Augments that change the same slot can't be held together. A shield needs the
// plain pistol, so it counts as changing slot 2.
const SLOT_ONE: [Augment; 3] = [Augment::Missile, Augment::Akimbo, Augment::Possession];
const SLOT_TWO: [Augment; 5] = [
    Augment::Shotgun,
    Augment::Magnum,
    Augment::PistolUpgrade,
    Augment::Shield,
    Augment::Taser,
];
const PRISM: [Augment; 5] = [
    Augment::Wallhack,
    Augment::NoRecoil,
    Augment::Borrow,
    Augment::Missile,
    Augment::Akimbo,
];

pub fn max_hp(base: i32, active: &[Augment]) -> i32 {
    if active.contains(&Augment::Vest) { VEST_MAX_HP } else { base }
}

fn pool(grade: AugmentGrade) -> &'static [Augment] {
    match grade {
        AugmentGrade::Silver => &SILVER,
        AugmentGrade::Gold => &GOLD,
        AugmentGrade::Prism => &PRISM,
        AugmentGrade::Unspecified => &[],
    }
}

/// 40% silver, 35% gold, 25% prism.
pub fn roll_grade(rng: &mut GameRng) -> AugmentGrade {
    let roll = rng.next_f32();
    if roll < 0.4 {
        AugmentGrade::Silver
    } else if roll < 0.75 {
        AugmentGrade::Gold
    } else {
        AugmentGrade::Prism
    }
}

/// One card nobody has seen yet, recorded in `seen`. A grade with nothing left falls
/// back to the next one that has, which only matters once most of the pool is gone.
pub fn deal_one(rng: &mut GameRng, seen: &mut Vec<Augment>) -> Option<Augment> {
    let first = roll_grade(rng);
    let order = [first, AugmentGrade::Silver, AugmentGrade::Gold, AugmentGrade::Prism];
    let card = order.iter().find_map(|&grade| {
        let fresh: Vec<Augment> = pool(grade).iter().copied().filter(|a| !seen.contains(a)).collect();
        (!fresh.is_empty()).then(|| fresh[rng.index(fresh.len())])
    })?;
    seen.push(card);
    Some(card)
}

/// Deals a hand of cards, each rolling its own grade.
pub fn deal(rng: &mut GameRng, seen: &mut Vec<Augment>) -> Vec<Augment> {
    (0..HAND_SIZE).filter_map(|_| deal_one(rng, seen)).collect()
}

/// Double Time's two augments, drawn at the usual odds without clashing.
pub fn roll_double(rng: &mut GameRng) -> Vec<Augment> {
    let mut seen = vec![Augment::DoubleTime];
    let Some(first) = deal_one(rng, &mut seen) else {
        return Vec::new();
    };
    for group in [&SLOT_ONE[..], &SLOT_TWO[..]] {
        if group.contains(&first) {
            seen.extend_from_slice(group);
        }
    }
    let mut active = vec![first];
    active.extend(deal_one(rng, &mut seen));
    active
}
