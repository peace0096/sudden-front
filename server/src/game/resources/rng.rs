//! The match's seeded random numbers.

use bevy_ecs::prelude::*;

/// Small seeded xorshift so matches and tests are reproducible.
#[derive(Resource)]
pub struct GameRng(pub u64);

impl GameRng {
    pub fn next_f32(&mut self) -> f32 {
        let mut x = self.0;
        x ^= x << 13;
        x ^= x >> 7;
        x ^= x << 17;
        self.0 = x;
        (x >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next_f32()
    }

    pub fn index(&mut self, len: usize) -> usize {
        ((self.next_f32() * len as f32) as usize).min(len.saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_same_seed_gives_the_same_numbers_and_another_seed_others() {
        let draws = |seed: u64| {
            let mut rng = GameRng(seed);
            (0..100)
                .map(|_| (rng.next_f32(), rng.range(-1.0, 1.0), rng.index(7)))
                .collect::<Vec<_>>()
        };
        assert_eq!(draws(42), draws(42));
        assert_ne!(draws(42), draws(43));
    }

    #[test]
    fn draws_spread_evenly_from_zero_up_to_one() {
        let mut rng = GameRng(7);
        let mut tenths = [0u32; 10];
        for _ in 0..100_000 {
            let draw = rng.next_f32();
            assert!((0.0..1.0).contains(&draw), "{draw}");
            let tenth = (draw * 10.0) as usize;
            tenths[tenth] = tenths[tenth].saturating_add(1);
        }
        for count in tenths {
            assert!((9_000..11_000).contains(&count), "{tenths:?}");
        }
    }

    #[test]
    fn a_range_draw_stays_between_its_ends() {
        let mut rng = GameRng(7);
        for (min, max) in [(0.5, 1.4), (-0.085, 0.085), (5.0, 10.0), (2.0, 2.0)] {
            for _ in 0..10_000 {
                let draw = rng.range(min, max);
                assert!((min..=max).contains(&draw), "{draw} from {min} to {max}");
            }
        }
    }

    #[test]
    fn an_index_is_always_within_the_length_and_reaches_every_place() {
        let mut rng = GameRng(9);
        for len in [1, 2, 3, 7, 12] {
            let mut drawn = vec![false; len];
            for _ in 0..1_000 {
                let index = rng.index(len);
                assert!(index < len, "{index} of {len}");
                drawn[index] = true;
            }
            assert!(drawn.iter().all(|&d| d), "{len}: {drawn:?}");
        }
        assert_eq!(
            rng.index(0),
            0,
            "an empty length gives 0, not a wrap below it"
        );
    }
}
