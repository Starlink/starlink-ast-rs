//! Random number generator — exact port of the C library's combined LCG
//! with Bays-Durham shuffle (the "ran2" algorithm from Numerical Recipes).

const NTAB: usize = 32;

// Generator constants
const A1: i64 = 40014;
const A2: i64 = 40692;
const M1: i64 = 2_147_483_563;
const M2: i64 = 2_147_483_399;
const Q1: i64 = 53668;
const Q2: i64 = 52774;
const R1: i64 = 12211;
const R2: i64 = 3791;
const NWARM: usize = 8;

/// Random number generator context for a MathMap.
///
/// Each MathMap has its own RNG state so that random sequences are
/// independent and reproducible given the same seed.
#[derive(Debug, Clone)]
pub(crate) struct MathMapRng {
    rand1: i64,
    rand2: i64,
    random_int: i64,
    table: [i64; NTAB],
    active: bool,
    pub seed: i32,
    pub seed_set: bool,
    // Gauss Box-Muller state
    gauss_saved: Option<f64>,
    // Static-like cached values (computed once)
    nrand: usize,
    scale0: f64,
    norm: f64,
}

impl MathMapRng {
    /// Create a new RNG context with the given seed.
    pub fn new(seed: i32, seed_set: bool) -> Self {
        Self {
            rand1: 0,
            rand2: 0,
            random_int: 0,
            table: [0; NTAB],
            active: false,
            seed,
            seed_set,
            gauss_saved: None,
            nrand: 0,
            scale0: 0.0,
            norm: 0.0,
        }
    }

    /// Create with a default (unpredictable) seed.
    pub fn with_default_seed() -> Self {
        // Use a combination of address + counter for unpredictability,
        // matching the C DefaultSeed approach
        let seed = default_seed();
        Self::new(seed, false)
    }

    /// Produce a uniformly distributed pseudo-random number in [0, 1].
    pub fn rand(&mut self) -> f64 {
        // If not yet initialised, do so now.
        if !self.active {
            self.initialise();
        }

        let mut result = 0.0_f64;
        let mut scale = self.norm;

        for _irand in 0..self.nrand {
            // Update rand1 from (a1 * rand1) % m1
            let tmp = self.rand1 / Q1;
            self.rand1 = A1 * (self.rand1 - tmp * Q1) - tmp * R1;
            if self.rand1 < 0 {
                self.rand1 += M1;
            }

            // Update rand2 from (a2 * rand2) % m2
            let tmp = self.rand2 / Q2;
            self.rand2 = A2 * (self.rand2 - tmp * Q2) - tmp * R2;
            if self.rand2 < 0 {
                self.rand2 += M2;
            }

            // Use previous random_int to index shuffle table
            let mut itab = (self.random_int / (1 + (M1 - 1) / NTAB as i64)) as i32;
            if itab < 0 {
                itab = -itab;
            }
            let itab = (itab as usize) % NTAB;

            // Bays-Durham shuffle
            self.random_int = self.table[itab];
            self.table[itab] = self.rand1;

            // Combine with rand2
            self.random_int -= self.rand2;
            if self.random_int < 1 {
                self.random_int += M1 - 1;
            }

            // Accumulate contribution
            scale *= self.scale0;
            result += scale * (self.random_int - 1) as f64;
        }

        result
    }

    /// Produce a standard Gaussian (mean=0, sd=1) sample using Box-Muller.
    pub fn gauss(&mut self) -> f64 {
        // If context was just reinitialised, discard saved value
        if !self.active {
            self.gauss_saved = None;
        }

        if let Some(saved) = self.gauss_saved.take() {
            return saved;
        }

        loop {
            let x = 2.0 * self.rand() - 1.0;
            let y = 2.0 * self.rand() - 1.0;
            let rsq = x * x + y * y;
            if rsq >= 1.0 || rsq == 0.0 {
                continue;
            }
            let s = -2.0 * crate::math::ln(rsq);
            if f64::MAX * rsq >= s {
                let s = crate::math::sqrt(s / rsq);
                self.gauss_saved = Some(y * s);
                return x * s;
            }
        }
    }

    /// Produce a Poisson-distributed random sample with given mean.
    pub fn poisson(&mut self, mean: f64) -> f64 {
        use crate::constants::BAD_VALUE;

        const SMALL: f64 = 9.3;

        if mean < 0.0 {
            return BAD_VALUE;
        }
        if mean == 0.0 {
            return 0.0;
        }

        if mean <= SMALL {
            // Direct method
            let thresh = crate::math::exp(-mean);
            let mut product = 1.0;
            let mut result = -1.0;
            loop {
                product *= self.rand();
                result += 1.0;
                if product <= thresh {
                    break;
                }
            }
            result
        } else {
            let huge = crate::math::powi(1.0e3_f64 / f64::EPSILON, 2);
            if mean > huge {
                return mean;
            }

            // Rejection method
            let pi = std::f64::consts::PI;
            let sqrt_point9 = crate::math::sqrt(0.9_f64);
            let ranmax = (crate::math::sqrt(f64::MAX - 0.9) / crate::math::sqrt(0.9_f64))
                * (1.0 - 4.0 * f64::EPSILON);
            let log_mean = crate::math::ln(mean);
            let root_2mean = crate::math::sqrt(2.0 * mean);
            let beta = mean * log_mean - log_gamma(mean + 1.0);

            loop {
                let ran = loop {
                    let r = crate::math::tan(pi * self.rand());
                    if r <= ranmax && !r.is_infinite() {
                        break r;
                    }
                };

                let result = ran * root_2mean + mean;
                if result < 0.0 {
                    continue;
                }
                let result = result.floor();

                let ran_scaled = ran * sqrt_point9;
                let pfract = (0.9 + ran_scaled * ran_scaled)
                    * crate::math::exp(result * log_mean - log_gamma(result + 1.0) - beta);

                if self.rand() <= pfract {
                    return result;
                }
            }
        }
    }

    /// Reset the seed, marking the generator as needing reinitialisation.
    pub fn set_seed(&mut self, seed: i32) {
        self.seed = seed;
        self.seed_set = true;
        self.active = false;
        self.gauss_saved = None;
    }

    /// Clear the seed back to a default unpredictable value.
    pub fn clear_seed(&mut self) {
        self.seed = default_seed();
        self.seed_set = false;
        self.active = false;
        self.gauss_saved = None;
    }

    fn initialise(&mut self) {
        // Compute static-like values
        let lbits = {
            let (_, exp) = frexp(M1 as f64);
            exp
        };
        // f64 has 53 mantissa digits, radix 2
        let dbits = 53_usize;
        self.nrand = dbits.div_ceil(lbits as usize);
        self.scale0 = 1.0 / (M1 - 1) as f64;

        let mut sum = 0.0;
        let mut scale = 1.0;
        for _ in 0..self.nrand {
            scale *= self.scale0;
            sum += scale;
        }
        self.norm = 1.0 / (sum * (M1 - 2) as f64);

        // Enforce positive seed
        let mut seed = self.seed as i64;
        if seed < 1 {
            seed += i64::MAX;
        }
        if seed < 1 {
            seed = i64::MAX;
        }

        self.rand1 = seed;
        self.rand2 = seed;

        // Warm up and fill shuffle table
        for itab in (0..(NTAB + NWARM)).rev() {
            let tmp = self.rand1 / Q1;
            self.rand1 = A1 * (self.rand1 - tmp * Q1) - tmp * R1;
            if self.rand1 < 0 {
                self.rand1 += M1;
            }
            if itab < NTAB {
                self.table[itab] = self.rand1;
            }
        }

        self.random_int = self.table[0];
        self.active = true;
    }
}

/// Compute a default seed value (unpredictable).
fn default_seed() -> i32 {
    use std::time::SystemTime;
    let t = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .unwrap_or_default()
        .as_nanos();
    // Mix bits to get something reasonable
    ((t ^ (t >> 32)) & 0x7FFF_FFFF) as i32
}

/// Log-gamma function for Poisson distribution calculation.
fn log_gamma(x: f64) -> f64 {
    // Stirling approximation coefficients (from Numerical Recipes)
    const COF: [f64; 6] = [
        76.180_091_729_471_46,
        -86.505_320_329_416_77,
        24.014_098_240_830_91,
        -1.231_739_572_450_155,
        0.001_208_650_973_866_179,
        -0.000_005_395_239_384_953,
    ];

    let mut y = x;
    let tmp = x + 5.5;
    let tmp = tmp - (x + 0.5) * crate::math::ln(tmp);
    let mut ser = 1.000_000_000_190_015;
    for &c in &COF {
        y += 1.0;
        ser += c / y;
    }
    -tmp + crate::math::ln(2.506_628_274_631_000_5 * ser / x)
}

/// frexp equivalent — decompose into fraction and exponent
fn frexp(x: f64) -> (f64, i32) {
    if x == 0.0 {
        return (0.0, 0);
    }
    let bits = x.to_bits();
    let exp = ((bits >> 52) & 0x7FF) as i32 - 1022;
    // Set exponent to -1 (biased: 1022) to get fraction in [0.5, 1.0)
    let frac_bits = (bits & 0x800F_FFFF_FFFF_FFFF) | (1022_u64 << 52);
    (f64::from_bits(frac_bits), exp)
}
