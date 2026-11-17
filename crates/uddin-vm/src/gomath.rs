//! The exponential, logarithm and power functions of the reference platform, bit for bit: the routines written in
//! assembly for amd64 and the portable code around them, with a fused multiply-add where the CPU has one. The
//! last bit of a result shows in what a script prints, and the library of the host does not give it. The `**`
//! operator and the builtins that name these functions are one implementation.

// The constants keep the digits of the routines they come from, more than a float holds.
#![allow(clippy::excessive_precision)]

pub const SMALLEST_NORMAL: f64 = 2.225_073_858_507_201_4e-308;

pub fn nan() -> f64 {
    f64::from_bits(0x7FF8_0000_0000_0001)
}

/// The fraction in [0.5, 1) and the power of two of a float.
pub fn frexp(f: f64) -> (f64, i64) {
    if f == 0.0 || f.is_infinite() || f.is_nan() {
        return (f, 0);
    }
    let (f, mut exp) = normalize(f);
    let x = f.to_bits();
    exp += ((x >> 52) & 0x7FF) as i64 - 1023 + 1;
    let x = (x & !(0x7FFu64 << 52)) | (1022u64 << 52);
    (f64::from_bits(x), exp)
}

fn normalize(x: f64) -> (f64, i64) {
    if x.abs() < SMALLEST_NORMAL {
        (x * (1u64 << 52) as f64, -52)
    } else {
        (x, 0)
    }
}

/// `frac` times two to the power `exp`.
pub fn ldexp(frac: f64, exp: i64) -> f64 {
    if frac == 0.0 || frac.is_infinite() || frac.is_nan() {
        return frac;
    }
    let (frac, e) = normalize(frac);
    let mut exp = exp.saturating_add(e);
    let mut x = frac.to_bits();
    exp = exp.saturating_add(((x >> 52) & 0x7FF) as i64 - 1023);
    if exp < -1075 {
        return 0f64.copysign(frac);
    }
    if exp > 1023 {
        return if frac < 0.0 {
            f64::NEG_INFINITY
        } else {
            f64::INFINITY
        };
    }
    let mut m = 1.0;
    if exp < -1022 {
        exp += 53;
        m = 1.0 / (1u64 << 53) as f64;
    }
    x &= !(0x7FFu64 << 52);
    x |= ((exp + 1023) as u64) << 52;
    m * f64::from_bits(x)
}

pub fn modf(f: f64) -> (f64, f64) {
    let int = f.trunc();
    (int, (f - int).copysign(f))
}

/// `exp` as the amd64 build computes it with fused multiply-add: a range reduction by ln 2 and a Taylor
/// series.
pub fn exp(x: f64) -> f64 {
    const LN2U: f64 = 0.693_147_180_559_662_956_511_601_805_686_95;
    const LN2L: f64 = 0.282_352_905_630_315_771_225_884_481_750_134_36e-12;
    const OVERFLOW: f64 = 7.097_827_128_933_84e2;
    if x.is_nan() || x == f64::INFINITY {
        return x;
    }
    if x == f64::NEG_INFINITY {
        return 0.0;
    }
    if x > OVERFLOW {
        return f64::INFINITY;
    }
    let t = (x * std::f64::consts::LOG2_E).round_ties_even();
    let bx: i32 = if (-2_147_483_648.0..=2_147_483_647.0).contains(&t) {
        t as i32
    } else {
        i32::MIN
    };
    let k = f64::from(bx);
    let mut x0 = (-k).mul_add(LN2U, x);
    x0 = (-k).mul_add(LN2L, x0);
    x0 *= 0.0625;
    let mut x1 = 2.480_158_730_158_73e-5;
    for c in [
        1.984_126_984_126_984e-4,
        1.388_888_888_888_889e-3,
        8.333_333_333_333_333e-3,
        4.166_666_666_666_666_7e-2,
        1.666_666_666_666_666_6e-1,
        0.5,
        1.0,
    ] {
        x1 = x0.mul_add(x1, c);
    }
    x0 *= x1;
    for _ in 0..3 {
        x1 = x0 + 2.0;
        x0 *= x1;
    }
    x1 = x0 + 2.0;
    x0 = x1.mul_add(x0, 1.0);
    let b = i64::from(bx) + 0x3FF;
    if b <= 0 {
        if b < -52 {
            return 0.0;
        }
        let scale = f64::from_bits(((b + 0x3FE) as u64) << 52);
        return x0 * scale * f64::from_bits(1u64 << 52);
    }
    if b >= 0x7FF {
        return f64::INFINITY;
    }
    x0 * f64::from_bits((b as u64) << 52)
}

/// The natural logarithm as the assembly of the amd64 build does it. It takes the exponent from the bits
/// without normalizing, so a subnormal argument gives a result that is not the logarithm, and it swaps to the
/// next binade when the fraction is not above the square root of a half.
pub fn log(x: f64) -> f64 {
    const HALF_SQRT2: f64 = std::f64::consts::FRAC_1_SQRT_2;
    const LN2_HI: f64 = 6.931_471_803_691_238_164_90e-01;
    const LN2_LO: f64 = 1.908_214_929_270_587_700_02e-10;
    const L1: f64 = 6.666_666_666_666_735_130e-01;
    const L2: f64 = 3.999_999_999_940_941_908e-01;
    const L3: f64 = 2.857_142_874_366_239_149e-01;
    const L4: f64 = 2.222_219_843_214_978_396e-01;
    const L5: f64 = 1.818_357_216_161_805_012e-01;
    const L6: f64 = 1.531_383_769_920_937_332e-01;
    const L7: f64 = 1.479_819_860_511_658_591e-01;
    let bits = x.to_bits();
    if bits & !(1 << 63) == 0 {
        return f64::NEG_INFINITY;
    }
    if (bits as i64) < 0 {
        return nan();
    }
    if bits >= 0x7FF0_0000_0000_0000 {
        return x;
    }
    let mut f1 = f64::from_bits((bits & 0x000F_FFFF_FFFF_FFFF) | 0x3FE0_0000_0000_0000);
    let mut k = (((bits >> 52) & 0x7FF) as i64 - 0x3FE) as f64;
    if HALF_SQRT2 >= f1 {
        k -= 1.0;
        f1 *= 2.0;
    }
    let f = f1 - 1.0;
    let s = f / (2.0 + f);
    let s2 = s * s;
    let s4 = s2 * s2;
    let t1 = s2 * (L1 + s4 * (L3 + s4 * (L5 + s4 * L7)));
    let t2 = s4 * (L2 + s4 * (L4 + s4 * L6));
    let r = t1 + t2;
    let hfsq = 0.5 * f * f;
    k * LN2_HI - ((hfsq - (s * (hfsq + r) + k * LN2_LO)) - f)
}

fn is_odd_int(x: f64) -> bool {
    if x.abs() >= (1u64 << 53) as f64 {
        return false;
    }
    let (xi, xf) = modf(x);
    xf == 0.0 && (xi as i64) & 1 == 1
}

/// `x` to the power `y`.
pub fn pow(x: f64, y: f64) -> f64 {
    if y == 0.0 || x == 1.0 {
        return 1.0;
    }
    if y == 1.0 {
        return x;
    }
    if x.is_nan() || y.is_nan() {
        return nan();
    }
    if x == 0.0 {
        if y < 0.0 {
            return if x.is_sign_negative() && is_odd_int(y) {
                f64::NEG_INFINITY
            } else {
                f64::INFINITY
            };
        }
        if y > 0.0 {
            return if x.is_sign_negative() && is_odd_int(y) {
                x
            } else {
                0.0
            };
        }
    } else if y.is_infinite() {
        if x == -1.0 {
            return 1.0;
        }
        return if (x.abs() < 1.0) == (y > 0.0) {
            0.0
        } else {
            f64::INFINITY
        };
    } else if x.is_infinite() {
        if x < 0.0 {
            return pow(1.0 / x, -y);
        }
        if y < 0.0 {
            return 0.0;
        }
        if y > 0.0 {
            return f64::INFINITY;
        }
    } else if y == 0.5 {
        return x.sqrt();
    } else if y == -0.5 {
        return 1.0 / x.sqrt();
    }
    let (mut yi, mut yf) = modf(y.abs());
    if yf != 0.0 && x < 0.0 {
        return nan();
    }
    if yi >= 9.223_372_036_854_775_808e18 {
        if x == -1.0 {
            return 1.0;
        }
        return if (x.abs() < 1.0) == (y > 0.0) {
            0.0
        } else {
            f64::INFINITY
        };
    }
    let mut a1 = 1.0;
    let mut ae: i64 = 0;
    if yf != 0.0 {
        if yf > 0.5 {
            yf -= 1.0;
            yi += 1.0;
        }
        a1 = exp(yf * log(x));
    }
    let (mut x1, mut xe) = frexp(x);
    let mut i = yi as i64;
    while i != 0 {
        if !(-(1 << 12)..=(1 << 12)).contains(&xe) {
            ae += xe;
            break;
        }
        if i & 1 == 1 {
            a1 *= x1;
            ae += xe;
        }
        x1 *= x1;
        xe <<= 1;
        if x1 < 0.5 {
            x1 += x1;
            xe -= 1;
        }
        i >>= 1;
    }
    if y < 0.0 {
        a1 = 1.0 / a1;
        ae = -ae;
    }
    ldexp(a1, ae)
}
