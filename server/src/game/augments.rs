//! Augment mode: the card pool and how hands are dealt.

use super::resources::GameRng;
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
/// Added to a victim's head size per Big Head holder who helped kill them, up to BIG_HEAD_MAX
/// (client/src/game/art/hitbox.ts HEAD_SCALE_MAX keeps a copy for the character's bounds).
pub const BIG_HEAD_STEP: f32 = 0.25;
pub const BIG_HEAD_MAX: f32 = 3.0;

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
    if active.contains(&Augment::Vest) {
        VEST_MAX_HP
    } else {
        base
    }
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
    let order = [
        first,
        AugmentGrade::Silver,
        AugmentGrade::Gold,
        AugmentGrade::Prism,
    ];
    let card = order.iter().find_map(|&grade| {
        let fresh: Vec<Augment> = pool(grade)
            .iter()
            .copied()
            .filter(|a| !seen.contains(a))
            .collect();
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::game::systems::weapon::forced_primary;
    use crate::game::test_support::*;
    use crate::game::weapons::{primary_for, sidearm_for, PRIMARIES};

    const GRADES: [AugmentGrade; 3] = [
        AugmentGrade::Silver,
        AugmentGrade::Gold,
        AugmentGrade::Prism,
    ];

    /// Every augment there is.
    fn every_augment() -> impl Iterator<Item = Augment> {
        (0..64)
            .filter_map(|n| Augment::try_from(n).ok())
            .filter(|&augment| augment != Augment::Unspecified)
    }

    #[test]
    fn every_augment_is_dealt_from_exactly_one_grade() {
        for augment in every_augment() {
            let grades = GRADES
                .iter()
                .filter(|&&grade| pool(grade).contains(&augment))
                .count();
            assert_eq!(grades, 1, "{augment:?}");
        }
        assert!(pool(AugmentGrade::Unspecified).is_empty());
    }

    #[test]
    fn every_augment_that_changes_a_weapon_slot_clashes_with_the_others_there() {
        for augment in every_augment() {
            let changes_slot_one = forced_primary(&[augment]).is_some()
                || PRIMARIES
                    .iter()
                    .any(|&kind| primary_for(kind, &[augment]) != kind);
            let changes_slot_two = sidearm_for(&[augment]) != Kind::Pistol;
            assert!(
                !changes_slot_one || SLOT_ONE.contains(&augment),
                "{augment:?} changes slot 1"
            );
            assert!(
                !changes_slot_two || SLOT_TWO.contains(&augment),
                "{augment:?} changes slot 2"
            );
        }
        // A shield needs the plain pistol, so it holds slot 2 too.
        assert!(SLOT_TWO.contains(&Augment::Shield));
        assert!(SLOT_ONE.iter().all(|augment| !SLOT_TWO.contains(augment)));
    }

    #[test]
    fn dealing_one_card_at_a_time_gives_every_augment_once_and_then_nothing() {
        let total = every_augment().count();
        let mut rng = GameRng(5);
        let mut seen = Vec::new();
        let mut dealt = Vec::new();
        while let Some(card) = deal_one(&mut rng, &mut seen) {
            assert!(!dealt.contains(&card), "{card:?} dealt twice");
            dealt.push(card);
            assert!(dealt.len() <= total, "never ran out");
        }
        assert_eq!(dealt.len(), total);
        assert_eq!(seen, dealt, "each card is recorded as it is dealt");
        assert_eq!(deal_one(&mut rng, &mut seen), None);
        assert_eq!(seen.len(), total);
    }

    #[test]
    fn a_grade_that_has_run_out_falls_back_to_one_that_has_not() {
        let mut rng = GameRng(11);
        for _ in 0..100 {
            let mut seen: Vec<Augment> = SILVER.iter().chain(&GOLD).copied().collect();
            let card = deal_one(&mut rng, &mut seen);
            assert!(card.is_some_and(|card| PRISM.contains(&card)), "{card:?}");
        }
    }

    #[test]
    fn a_hand_is_cards_nobody_has_seen_which_count_as_seen_from_then_on() {
        let mut rng = GameRng(3);
        let mut seen = vec![Augment::Vest, Augment::Missile];
        let before = seen.clone();
        let first = deal(&mut rng, &mut seen);
        assert_eq!(first.len(), HAND_SIZE);
        for (i, card) in first.iter().enumerate() {
            assert!(!before.contains(card), "{card:?} was seen already");
            assert!(!first[..i].contains(card), "{card:?} twice in {first:?}");
        }
        assert_eq!(seen[..before.len()], before[..]);
        assert_eq!(seen[before.len()..], first[..]);
        let second = deal(&mut rng, &mut seen);
        assert!(
            second.iter().all(|card| !first.contains(card)),
            "{first:?} then {second:?}"
        );
    }

    #[test]
    fn a_hand_runs_short_only_once_fewer_cards_than_a_hand_are_left() {
        let all: Vec<Augment> = every_augment().collect();
        let mut rng = GameRng(3);
        let mut seen = all[2..].to_vec();
        let mut hand = deal(&mut rng, &mut seen);
        hand.sort_unstable();
        assert_eq!(hand, all[..2]);
        assert!(deal(&mut rng, &mut seen).is_empty());
    }

    #[test]
    fn double_time_can_pair_augments_that_change_different_slots() {
        let mut rng = GameRng(21);
        let paired = (0..5_000).any(|_| {
            let active = roll_double(&mut rng);
            active.iter().any(|a| SLOT_ONE.contains(a))
                && active.iter().any(|a| SLOT_TWO.contains(a))
        });
        assert!(paired, "slot 1 and slot 2 augments never came together");
    }

    #[test]
    fn the_vest_sets_the_max_hp_whatever_the_base_and_no_other_augment_changes_it() {
        for base in [MAX_HP, AUGMENT_MAX_HP] {
            assert_eq!(max_hp(base, &[]), base);
            assert_eq!(max_hp(base, &[Augment::Vest]), VEST_MAX_HP);
            assert_eq!(max_hp(base, &[Augment::Rest, Augment::Vest]), VEST_MAX_HP);
            for augment in every_augment().filter(|&a| a != Augment::Vest) {
                assert_eq!(max_hp(base, &[augment]), base, "{augment:?}");
            }
        }
        assert!(
            max_hp(AUGMENT_MAX_HP, &[Augment::Vest]) > AUGMENT_MAX_HP,
            "the vest adds health"
        );
    }

    #[test]
    fn grades_roll_at_40_35_25() {
        let mut rng = GameRng(12345);
        let mut counts = [0u32; 3];
        let total = 100_000;
        for _ in 0..total {
            counts[augments::roll_grade(&mut rng) as usize - 1] += 1;
        }
        for (count, expected) in counts.iter().zip([0.4, 0.35, 0.25]) {
            let share = *count as f64 / total as f64;
            assert!((share - expected).abs() < 0.01, "{counts:?}");
        }
    }

    #[test]
    fn double_time_draws_two_that_dont_clash_at_the_usual_odds() {
        let slot_one = [Aug::Missile, Aug::Akimbo, Aug::Possession];
        let slot_two = [
            Aug::Shotgun,
            Aug::Magnum,
            Aug::PistolUpgrade,
            Aug::Shield,
            Aug::Taser,
        ];
        let mut rng = GameRng(99);
        let mut prism = 0;
        let total = 20_000;
        for _ in 0..total {
            let active = augments::roll_double(&mut rng);
            assert_eq!(active.len(), 2);
            assert_ne!(active[0], active[1]);
            assert!(!active.contains(&Aug::DoubleTime));
            for group in [&slot_one[..], &slot_two[..]] {
                assert!(
                    active.iter().filter(|a| group.contains(a)).count() <= 1,
                    "{active:?}"
                );
            }
            if [
                Aug::Wallhack,
                Aug::NoRecoil,
                Aug::Borrow,
                Aug::Missile,
                Aug::Akimbo,
            ]
            .contains(&active[0])
            {
                prism += 1;
            }
        }
        let share = prism as f64 / total as f64;
        assert!(
            (share - 0.25).abs() < 0.02,
            "first draw prism share {share}"
        );
    }
}
