//! A small seeded random source, so a run needs no extra crate and a seed replays a bot's choices.

pub struct Rng(u64);

impl Rng {
    pub fn new(seed: u64) -> Self {
        // Zero would stay zero forever.
        Self(seed | 1)
    }

    /// A seed from the clock, mixed with `salt` so bots started together still differ.
    pub fn from_clock(salt: u64) -> Self {
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map(|d| d.as_nanos() as u64)
            .unwrap_or(0x9E37_79B9_7F4A_7C15);
        Self::new(now ^ salt.wrapping_mul(0x9E37_79B9_7F4A_7C15))
    }

    pub fn next_u64(&mut self) -> u64 {
        // xorshift64*
        let mut x = self.0;
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        self.0 = x;
        x.wrapping_mul(0x2545_F491_4F6C_DD1D)
    }

    /// Uniform in [0, 1).
    pub fn next_f32(&mut self) -> f32 {
        (self.next_u64() >> 40) as f32 / (1u64 << 24) as f32
    }

    pub fn range(&mut self, min: f32, max: f32) -> f32 {
        min + (max - min) * self.next_f32()
    }

    /// Uniform in 0..len; 0 when `len` is 0.
    pub fn index(&mut self, len: usize) -> usize {
        self.next_u64().checked_rem(len as u64).unwrap_or(0) as usize
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn floats_stay_within_their_range() {
        let mut rng = Rng::new(7);
        for _ in 0..10_000 {
            let unit = rng.next_f32();
            assert!((0.0..1.0).contains(&unit), "{unit}");
            let ranged = rng.range(-2.0, 3.0);
            assert!((-2.0..3.0).contains(&ranged), "{ranged}");
        }
    }

    #[test]
    fn an_index_stays_below_its_length_and_an_empty_one_is_zero() {
        let mut rng = Rng::new(11);
        for _ in 0..1_000 {
            assert!(rng.index(5) < 5);
        }
        assert_eq!(rng.index(0), 0);
    }

    #[test]
    fn the_same_seed_gives_the_same_numbers() {
        let (mut a, mut b) = (Rng::new(42), Rng::new(42));
        for _ in 0..100 {
            assert_eq!(a.next_u64(), b.next_u64());
        }
    }
}
