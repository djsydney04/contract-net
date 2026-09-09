//! Reference-compatible MT19937 integer seeding, 53-bit floats, and rejection sampling.
//! Algorithm provenance and license: see docs/THIRD_PARTY_NOTICES.md.
use num_bigint::BigUint;
use num_traits::Zero;

pub(crate) struct PythonRandom {
    state: [u32; 624],
    index: usize,
}

impl PythonRandom {
    pub(crate) fn new(seed: &BigUint) -> Self {
        let mut key = seed.to_u32_digits();
        if key.is_empty() {
            key.push(0);
        }
        let mut state = [0u32; 624];
        state[0] = 19_650_218;
        for i in 1..624 {
            state[i] = 1_812_433_253u32
                .wrapping_mul(state[i - 1] ^ (state[i - 1] >> 30))
                .wrapping_add(i as u32);
        }
        let (mut i, mut j) = (1, 0);
        for _ in 0..624.max(key.len()) {
            state[i] = (state[i] ^ (state[i - 1] ^ (state[i - 1] >> 30)).wrapping_mul(1_664_525))
                .wrapping_add(key[j])
                .wrapping_add(j as u32);
            i += 1;
            j += 1;
            if i == 624 {
                state[0] = state[623];
                i = 1;
            }
            if j == key.len() {
                j = 0;
            }
        }
        for _ in 0..623 {
            state[i] = (state[i]
                ^ (state[i - 1] ^ (state[i - 1] >> 30)).wrapping_mul(1_566_083_941))
            .wrapping_sub(i as u32);
            i += 1;
            if i == 624 {
                state[0] = state[623];
                i = 1;
            }
        }
        state[0] = 0x8000_0000;
        Self { state, index: 624 }
    }

    pub(crate) fn next_u32(&mut self) -> u32 {
        if self.index == 624 {
            for i in 0..624 {
                let y = (self.state[i] & 0x8000_0000) | (self.state[(i + 1) % 624] & 0x7fff_ffff);
                self.state[i] = self.state[(i + 397) % 624]
                    ^ (y >> 1)
                    ^ if y & 1 == 1 { 0x9908_b0df } else { 0 };
            }
            self.index = 0;
        }
        let mut y = self.state[self.index];
        self.index += 1;
        y ^= y >> 11;
        y ^= (y << 7) & 0x9d2c_5680;
        y ^= (y << 15) & 0xefc6_0000;
        y ^= y >> 18;
        y
    }

    pub(crate) fn random(&mut self) -> f64 {
        let a = self.next_u32() >> 5;
        let b = self.next_u32() >> 6;
        (f64::from(a) * 67_108_864.0 + f64::from(b)) / 9_007_199_254_740_992.0
    }

    pub(crate) fn below_u64(&mut self, upper: u64) -> u64 {
        debug_assert!(upper > 0);
        let bits = 64 - upper.leading_zeros();
        loop {
            let value = if bits <= 32 {
                u64::from(self.next_u32() >> (32 - bits))
            } else {
                let low = u64::from(self.next_u32());
                let high = u64::from(self.next_u32() >> (64 - bits));
                low | (high << 32)
            };
            if value < upper {
                return value;
            }
        }
    }

    pub(crate) fn below_big(&mut self, upper: &BigUint) -> BigUint {
        debug_assert!(!upper.is_zero());
        let bits = upper.bits();
        loop {
            let mut words = Vec::with_capacity(bits.div_ceil(32) as usize);
            let mut remaining = bits;
            while remaining > 0 {
                let used = remaining.min(32) as u32;
                words.push(self.next_u32() >> (32 - used));
                remaining -= u64::from(used);
            }
            let value = BigUint::new(words);
            if &value < upper {
                return value;
            }
        }
    }
}
