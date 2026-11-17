//! A small seeded generator. Its output is fixed by the seed alone, so a case can be rebuilt from its id.

pub struct Rng(u64);

fn mix(mut z: u64) -> u64 {
    z = (z ^ (z >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    z = (z ^ (z >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    z ^ (z >> 31)
}

impl Rng {
    /// The generator of case `index` of a run with `seed`: cases do not depend on each other.
    pub fn for_case(seed: u64, index: u64) -> Rng {
        Rng(mix(seed.wrapping_mul(0x9e37_79b9_7f4a_7c15) ^ mix(index)))
    }

    pub fn next_u64(&mut self) -> u64 {
        self.0 = self.0.wrapping_add(0x9e37_79b9_7f4a_7c15);
        mix(self.0)
    }

    /// 0..n; n must be above 0.
    pub fn below(&mut self, n: usize) -> usize {
        (self.next_u64() % n as u64) as usize
    }

    /// lo..=hi
    pub fn range(&mut self, lo: usize, hi: usize) -> usize {
        lo + self.below(hi - lo + 1)
    }

    pub fn chance(&mut self, percent: u32) -> bool {
        self.next_u64() % 100 < u64::from(percent)
    }

    pub fn pick<'a, T>(&mut self, items: &'a [T]) -> &'a T {
        &items[self.below(items.len())]
    }

    /// An index chosen with the given weights.
    pub fn weighted(&mut self, weights: &[u32]) -> usize {
        let total: u64 = weights.iter().map(|&w| u64::from(w)).sum();
        let mut roll = self.next_u64() % total;
        for (i, &w) in weights.iter().enumerate() {
            if roll < u64::from(w) {
                return i;
            }
            roll -= u64::from(w);
        }
        weights.len() - 1
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn same_seed_same_stream_and_cases_differ() {
        assert_eq!(
            Rng::for_case(1, 7).next_u64(),
            Rng::for_case(1, 7).next_u64()
        );
        assert_ne!(
            Rng::for_case(1, 7).next_u64(),
            Rng::for_case(1, 8).next_u64()
        );
        assert_ne!(
            Rng::for_case(1, 7).next_u64(),
            Rng::for_case(2, 7).next_u64()
        );
    }
}
