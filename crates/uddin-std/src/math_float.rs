//! Math, float functions: acos asin atan atan2 cbrt cos cosh exp exp2 log log10 log2 logb pow radians degrees sin sinh sqrt tan tanh ceil floor round trunc is_nan is_infinite.
//!
//! The last bit of a result shows in what a script prints, so the functions are the ones of the Go math library
//! on amd64, step by step: Cephes polynomials for the circular and hyperbolic functions, and for `exp` and `log`
//! the routines written in assembly, with a fused multiply-add where the CPU has one. The standard library's
//! results differ from these in the last bit for some arguments.

// The constants keep the digits of the library they come from, more than a float holds.
#![allow(clippy::excessive_precision)]

use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, LOG2_E, LOG10_E, PI};

use uddin_vm::gomath::{self, SMALLEST_NORMAL, frexp, ldexp, nan};
use uddin_vm::{BuiltinDef, Exc, Rt, Value, float_to_int};

use crate::math_stats::{not_a_number, number};

pub(crate) const TABLE: &[BuiltinDef] = &[
    BuiltinDef::new("acos", acos, 1).fast_path(),
    BuiltinDef::new("asin", asin, 1).fast_path(),
    BuiltinDef::new("atan", atan, 1).fast_path(),
    BuiltinDef::new("atan2", atan2, 2),
    BuiltinDef::new("cbrt", cbrt, 1).fast_path(),
    BuiltinDef::new("cos", cos, 1).fast_path(),
    BuiltinDef::new("cosh", cosh, 1).fast_path(),
    BuiltinDef::new("exp", exp, 1).fast_path(),
    BuiltinDef::new("exp2", exp2, 1).fast_path(),
    BuiltinDef::new("log", log, 1).fast_path(),
    BuiltinDef::new("log10", log10, 1).fast_path(),
    BuiltinDef::new("log2", log2, 1).fast_path(),
    BuiltinDef::new("logb", logb, 2),
    BuiltinDef::new("pow", pow_builtin, 2).fast_path(),
    BuiltinDef::new("radians", radians, 1).fast_path(),
    BuiltinDef::new("degrees", degrees, 1).fast_path(),
    BuiltinDef::new("sin", sin, 1).fast_path(),
    BuiltinDef::new("sinh", sinh, 1).fast_path(),
    BuiltinDef::new("sqrt", sqrt, 1).fast_path(),
    BuiltinDef::new("tan", tan, 1).fast_path(),
    BuiltinDef::new("tanh", tanh, 1).fast_path(),
    BuiltinDef::new("ceil", ceil, 1).fast_path(),
    BuiltinDef::new("floor", floor, 1).fast_path(),
    BuiltinDef::new("round", round, -1).fast_path(),
    BuiltinDef::new("trunc", trunc, 1).fast_path(),
    BuiltinDef::new("is_nan", is_nan, 1).fast_path(),
    BuiltinDef::new("is_infinite", is_infinite, 1).fast_path(),
];

/// The one number a builtin of one argument works on, or what it returns instead.
fn one_number(rt: &mut Rt<'_>, args: &[Value], name: &str) -> Result<Result<f64, Value>, Exc> {
    let [arg] = args else {
        return Err(rt.arity_error(name, 1, args.len()));
    };
    match number(*arg) {
        Some(x) => Ok(Ok(x)),
        None => not_a_number(rt, name, *arg).map(Err),
    }
}

/// A builtin that turns one number into a float, with the case in which it refuses the number.
fn unary(
    rt: &mut Rt<'_>,
    args: &[Value],
    name: &str,
    refuse: impl FnOnce(f64) -> Option<&'static str>,
    f: fn(f64) -> f64,
) -> Result<Value, Exc> {
    let x = match one_number(rt, args, name)? {
        Ok(x) => x,
        Err(error) => return Ok(error),
    };
    if let Some(message) = refuse(x) {
        return Err(Exc::value_error(rt.pos, message));
    }
    Ok(Value::Float(f(x)))
}

macro_rules! unary_builtins {
    ($($rust:ident $name:literal $refuse:expr, $f:path;)*) => {
        $(fn $rust(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
            unary(rt, args, $name, $refuse, $f)
        })*
    };
}

unary_builtins! {
    acos "acos" |x| (!(-1.0..=1.0).contains(&x) && !x.is_nan()).then_some("acos() input must be between -1 and 1"), arccosine;
    asin "asin" |x| (!(-1.0..=1.0).contains(&x) && !x.is_nan()).then_some("asin() input must be between -1 and 1"), arcsine;
    atan "atan" |_| None, arctangent;
    cbrt "cbrt" |_| None, cube_root;
    cos "cos" |_| None, cosine;
    cosh "cosh" |_| None, hyperbolic_cosine;
    exp "exp" |_| None, gomath::exp;
    exp2 "exp2" |_| None, exponential2;
    log "log" |x| (x <= 0.0).then_some("log() of non-positive number"), gomath::log;
    log10 "log10" |x| (x <= 0.0).then_some("log10() of non-positive number"), log_base10;
    log2 "log2" |x| (x <= 0.0).then_some("log2() of non-positive number"), log_base2;
    radians "radians" |_| None, to_radians;
    degrees "degrees" |_| None, to_degrees;
    sin "sin" |_| None, sine;
    sinh "sinh" |_| None, hyperbolic_sine;
    sqrt "sqrt" |x| (x < 0.0).then_some("sqrt() of negative number"), f64::sqrt;
    tan "tan" |_| None, tangent;
    tanh "tanh" |_| None, hyperbolic_tangent;
}

fn to_radians(degrees: f64) -> f64 {
    degrees * PI / 180.0
}

fn to_degrees(radians: f64) -> f64 {
    radians * 180.0 / PI
}

/// Two numbers, or the error value of the first that is not one.
fn two_numbers(
    rt: &mut Rt<'_>,
    args: &[Value],
    name: &str,
) -> Result<Result<(f64, f64), Value>, Exc> {
    let [a, b] = args else {
        return Err(rt.arity_error(name, 2, args.len()));
    };
    let Some(x) = number(*a) else {
        return not_a_number(rt, name, *a).map(Err);
    };
    let Some(y) = number(*b) else {
        return not_a_number(rt, name, *b).map(Err);
    };
    Ok(Ok((x, y)))
}

fn atan2(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    match two_numbers(rt, args, "atan2")? {
        Ok((y, x)) => Ok(Value::Float(arctangent2(y, x))),
        Err(error) => Ok(error),
    }
}

fn logb(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let (value, base) = match two_numbers(rt, args, "logb")? {
        Ok(pair) => pair,
        Err(error) => return Ok(error),
    };
    if value <= 0.0 {
        return Err(Exc::value_error(rt.pos, "logb() value must be positive"));
    }
    if base <= 0.0 || base == 1.0 {
        return Err(Exc::value_error(
            rt.pos,
            "logb() base must be positive and not equal to 1",
        ));
    }
    Ok(Value::Float(gomath::log(value) / gomath::log(base)))
}

/// `pow` of two integers is an integer when the result is whole.
fn pow_builtin(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    let (x, y) = match two_numbers(rt, args, "pow")? {
        Ok(pair) => pair,
        Err(error) => return Ok(error),
    };
    let result = gomath::pow(x, y);
    if matches!((args[0], args[1]), (Value::Int(_), Value::Int(_))) && result == result.trunc() {
        return Ok(Value::Int(float_to_int(result)));
    }
    Ok(Value::Float(result))
}

/// `ceil`, `floor` and `trunc`: the integer a float rounds to, the most negative one when it does not fit.
fn rounding(rt: &mut Rt<'_>, args: &[Value], name: &str, f: fn(f64) -> f64) -> Result<Value, Exc> {
    match one_number(rt, args, name)? {
        Ok(x) => Ok(Value::Int(float_to_int(f(x)))),
        Err(error) => Ok(error),
    }
}

fn ceil(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    rounding(rt, args, "ceil", f64::ceil)
}

fn floor(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    rounding(rt, args, "floor", f64::floor)
}

fn trunc(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    rounding(rt, args, "trunc", f64::trunc)
}

/// With one argument the integer it rounds to, half away from zero; with two, the float rounded to that many
/// places.
fn round(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    match args {
        [v] => {
            let Some(x) = number(*v) else {
                return not_a_number(rt, "round", *v);
            };
            Ok(Value::Int(float_to_int(round_half_away(x))))
        }
        [v, p] => {
            let Some(x) = number(*v) else {
                return not_a_number(rt, "round", *v);
            };
            let places = match *p {
                Value::Int(i) => i,
                Value::Float(f) => float_to_int(f),
                other => return not_a_number(rt, "round", other),
            };
            if places < 0 {
                return Err(Exc::value_error(
                    rt.pos,
                    "round() decimal places must not be negative",
                ));
            }
            let multiplier = gomath::pow(10.0, places as f64);
            Ok(Value::Float(round_half_away(x * multiplier) / multiplier))
        }
        _ => Err(rt.type_error(format!(
            "round() requires 1 or 2 arguments, got {}",
            args.len()
        ))),
    }
}

fn is_nan(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    match one_number(rt, args, "is_nan")? {
        Ok(x) => Ok(Value::Bool(x.is_nan())),
        Err(error) => Ok(error),
    }
}

fn is_infinite(rt: &mut Rt<'_>, args: &[Value]) -> Result<Value, Exc> {
    match one_number(rt, args, "is_infinite")? {
        Ok(x) => Ok(Value::Bool(x.is_infinite())),
        Err(error) => Ok(error),
    }
}

// ----- the library ---------------------------------------------------------------------------------------

/// Half away from zero.
pub(crate) fn round_half_away(x: f64) -> f64 {
    const HALF: u64 = 1 << 51;
    const FRAC_MASK: u64 = (1 << 52) - 1;
    let mut bits = x.to_bits();
    let mut e = (bits >> 52) & 0x7FF;
    if e < 1023 {
        bits &= 1 << 63;
        if e == 1022 {
            bits |= 0x3FF0_0000_0000_0000;
        }
    } else if e < 1023 + 52 {
        e -= 1023;
        bits += HALF >> e;
        bits &= !(FRAC_MASK >> e);
    }
    f64::from_bits(bits)
}

/// `exp` of two to a power, from the series of `exp_from_parts`.
fn exponential2(x: f64) -> f64 {
    const LN2_HI: f64 = 6.931_471_803_691_238_164_90e-01;
    const LN2_LO: f64 = 1.908_214_929_270_587_700_02e-10;
    const OVERFLOW: f64 = 1.023_999_999_999_999_9e3;
    const UNDERFLOW: f64 = -1.074_0e3;
    if x.is_nan() {
        return x;
    }
    if x > OVERFLOW {
        return f64::INFINITY;
    }
    if x < UNDERFLOW {
        return 0.0;
    }
    let k: i64 = if x > 0.0 {
        (x + 0.5) as i64
    } else if x < 0.0 {
        (x - 0.5) as i64
    } else {
        0
    };
    let t = x - k as f64;
    let hi = t * LN2_HI;
    let lo = -t * LN2_LO;
    exp_from_parts(hi, lo, k)
}

fn exp_from_parts(hi: f64, lo: f64, k: i64) -> f64 {
    const P1: f64 = 1.666_666_666_666_666_574_15e-01;
    const P2: f64 = -2.777_777_777_701_559_338_42e-03;
    const P3: f64 = 6.613_756_321_437_934_361_17e-05;
    const P4: f64 = -1.653_390_220_546_525_153_90e-06;
    const P5: f64 = 4.138_136_797_057_238_460_39e-08;
    let r = hi - lo;
    let t = r * r;
    let c = r - t * (P1 + t * (P2 + t * (P3 + t * (P4 + t * P5))));
    let y = 1.0 - ((lo - (r * c) / (2.0 - c)) - hi);
    ldexp(y, k)
}

fn log_base10(x: f64) -> f64 {
    gomath::log(x) * LOG10_E
}

fn log_base2(x: f64) -> f64 {
    let (frac, exp) = frexp(x);
    if frac == 0.5 {
        return (exp - 1) as f64;
    }
    gomath::log(frac) * LOG2_E + exp as f64
}

const PI4_A: f64 = 7.853_981_256_484_985_351_56e-1;
const PI4_B: f64 = 3.774_894_707_930_798_176_68e-8;
const PI4_C: f64 = 2.695_151_429_079_059_526_45e-15;
/// 4 / pi, rounded once.
const FOUR_OVER_PI: f64 = 1.273_239_544_735_162_8;
const REDUCE_THRESHOLD: f64 = (1u64 << 29) as f64;

const SIN: [f64; 6] = [
    1.589_623_015_765_465_680_60e-10,
    -2.505_074_776_285_780_728_66e-8,
    2.755_731_362_138_572_452_13e-6,
    -1.984_126_982_958_953_859_96e-4,
    8.333_333_333_322_118_588_78e-3,
    -1.666_666_666_666_663_072_95e-1,
];

const COS: [f64; 6] = [
    -1.135_853_652_138_768_173_00e-11,
    2.087_570_084_197_473_167_78e-9,
    -2.755_731_417_929_673_881_12e-7,
    2.480_158_728_885_170_453_48e-5,
    -1.388_888_888_887_305_641_16e-3,
    4.166_666_666_666_659_292_18e-2,
];

/// The octant and the remainder of `x` in it, for an argument at or above 2^29, from the digits of 4 / pi.
fn trig_reduce(x: f64) -> (u64, f64) {
    const MPI4: [u64; 20] = [
        0x0000_0000_0000_0001,
        0x45f3_06dc_9c88_2a53,
        0xf84e_afa3_ea69_bb81,
        0xb6c5_2b32_7887_2083,
        0xfca2_c757_bd77_8ac3,
        0x6e48_dc74_849b_a5c0,
        0x0c92_5dd4_13a3_2439,
        0xfc3b_d639_6253_4e7d,
        0xd104_6bea_5d76_8909,
        0xd338_e04d_68be_fc82,
        0x7323_ac73_06a6_73e9,
        0x3908_bf17_7bf2_5076,
        0x3ff1_2fff_bc0b_301f,
        0xde5e_2316_b414_da3e,
        0xda6c_fd9e_4f96_136e,
        0x9e8c_7ecd_3cbf_d45a,
        0xea4f_758f_d7cb_e2f6,
        0x7a0e_73ef_14a5_25d4,
        0xd7f6_bf62_3f1a_ba10,
        0xac06_608d_f8f6_d757,
    ];
    let shl = |v: u64, n: u32| if n >= 64 { 0 } else { v << n };
    let shr = |v: u64, n: u32| if n >= 64 { 0 } else { v >> n };
    if x < FRAC_PI_4 {
        return (0, x);
    }
    let mut ix = x.to_bits();
    let exp = ((ix >> 52) & 0x7FF) as i64 - 1023 - 52;
    ix &= !(0x7FFu64 << 52);
    ix |= 1 << 52;
    let digit = ((exp + 61) as u64 / 64) as usize;
    let bitshift = ((exp + 61) as u64 % 64) as u32;
    let z0 = shl(MPI4[digit], bitshift) | shr(MPI4[digit + 1], 64 - bitshift);
    let z1 = shl(MPI4[digit + 1], bitshift) | shr(MPI4[digit + 2], 64 - bitshift);
    let z2 = shl(MPI4[digit + 2], bitshift) | shr(MPI4[digit + 3], 64 - bitshift);
    let z2hi = ((u128::from(z2) * u128::from(ix)) >> 64) as u64;
    let z1prod = u128::from(z1) * u128::from(ix);
    let (z1hi, z1lo) = ((z1prod >> 64) as u64, z1prod as u64);
    let z0lo = z0.wrapping_mul(ix);
    let (lo, carry) = z1lo.overflowing_add(z2hi);
    let hi = z0lo.wrapping_add(z1hi).wrapping_add(u64::from(carry));
    let mut j = hi >> 61;
    let mut hi = hi << 3 | lo >> 61;
    let lz = hi.leading_zeros();
    let e = 1023 - (u64::from(lz) + 1);
    hi = shl(hi, lz + 1) | shr(lo, 64u32.wrapping_sub(lz + 1));
    hi >>= 64 - 52;
    hi |= e << 52;
    let mut z = f64::from_bits(hi);
    if j & 1 == 1 {
        j += 1;
        j &= 7;
        z -= 1.0;
    }
    (j, z * FRAC_PI_4)
}

fn sine(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        return x;
    }
    if x.is_infinite() {
        return nan();
    }
    let mut sign = false;
    let mut x = x;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    let (mut j, z);
    if x >= REDUCE_THRESHOLD {
        (j, z) = trig_reduce(x);
    } else {
        j = (x * FOUR_OVER_PI) as u64;
        let mut y = j as f64;
        if j & 1 == 1 {
            j += 1;
            y += 1.0;
        }
        j &= 7;
        z = ((x - y * PI4_A) - y * PI4_B) - y * PI4_C;
    }
    if j > 3 {
        sign = !sign;
        j -= 4;
    }
    let zz = z * z;
    let y = if j == 1 || j == 2 {
        1.0 - 0.5 * zz
            + zz * zz
                * ((((((COS[0] * zz) + COS[1]) * zz + COS[2]) * zz + COS[3]) * zz + COS[4]) * zz
                    + COS[5])
    } else {
        z + z
            * zz
            * ((((((SIN[0] * zz) + SIN[1]) * zz + SIN[2]) * zz + SIN[3]) * zz + SIN[4]) * zz
                + SIN[5])
    };
    if sign { -y } else { y }
}

fn cosine(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return nan();
    }
    let mut sign = false;
    let x = x.abs();
    let (mut j, z);
    if x >= REDUCE_THRESHOLD {
        (j, z) = trig_reduce(x);
    } else {
        j = (x * FOUR_OVER_PI) as u64;
        let mut y = j as f64;
        if j & 1 == 1 {
            j += 1;
            y += 1.0;
        }
        j &= 7;
        z = ((x - y * PI4_A) - y * PI4_B) - y * PI4_C;
    }
    if j > 3 {
        j -= 4;
        sign = !sign;
    }
    if j > 1 {
        sign = !sign;
    }
    let zz = z * z;
    let y = if j == 1 || j == 2 {
        z + z
            * zz
            * ((((((SIN[0] * zz) + SIN[1]) * zz + SIN[2]) * zz + SIN[3]) * zz + SIN[4]) * zz
                + SIN[5])
    } else {
        1.0 - 0.5 * zz
            + zz * zz
                * ((((((COS[0] * zz) + COS[1]) * zz + COS[2]) * zz + COS[3]) * zz + COS[4]) * zz
                    + COS[5])
    };
    if sign { -y } else { y }
}

fn tangent(x: f64) -> f64 {
    const P: [f64; 3] = [
        -1.309_369_391_813_837_776_46e4,
        1.153_516_648_385_874_161_40e6,
        -1.795_652_519_764_848_779_88e7,
    ];
    const Q: [f64; 5] = [
        1.0,
        1.368_129_634_706_929_546_78e4,
        -1.320_892_344_402_109_674_47e6,
        2.500_838_018_233_579_158_39e7,
        -5.386_957_559_294_546_298_81e7,
    ];
    if x == 0.0 || x.is_nan() {
        return x;
    }
    if x.is_infinite() {
        return nan();
    }
    let mut sign = false;
    let mut x = x;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    let (mut j, z);
    if x >= REDUCE_THRESHOLD {
        (j, z) = trig_reduce(x);
    } else {
        j = (x * FOUR_OVER_PI) as u64;
        let mut y = j as f64;
        if j & 1 == 1 {
            j += 1;
            y += 1.0;
        }
        z = ((x - y * PI4_A) - y * PI4_B) - y * PI4_C;
    }
    let zz = z * z;
    let mut y = if zz > 1e-14 {
        z + z
            * (zz * (((P[0] * zz) + P[1]) * zz + P[2])
                / ((((zz + Q[1]) * zz + Q[2]) * zz + Q[3]) * zz + Q[4]))
    } else {
        z
    };
    if j & 2 == 2 {
        y = -1.0 / y;
    }
    if sign { -y } else { y }
}

fn atan_core(x: f64) -> f64 {
    const P0: f64 = -8.750_608_600_031_904_122_785e-01;
    const P1: f64 = -1.615_753_718_733_365_076_637e+01;
    const P2: f64 = -7.500_855_792_314_704_667_340e+01;
    const P3: f64 = -1.228_866_684_490_136_173_410e+02;
    const P4: f64 = -6.485_021_904_942_025_371_773e+01;
    const Q0: f64 = 2.485_846_490_142_306_297_962e+01;
    const Q1: f64 = 1.650_270_098_316_988_542_046e+02;
    const Q2: f64 = 4.328_810_604_912_902_668_951e+02;
    const Q3: f64 = 4.853_903_996_359_136_964_868e+02;
    const Q4: f64 = 1.945_506_571_482_613_964_425e+02;
    let mut z = x * x;
    z = z * ((((P0 * z + P1) * z + P2) * z + P3) * z + P4)
        / (((((z + Q0) * z + Q1) * z + Q2) * z + Q3) * z + Q4);
    x * z + x
}

fn atan_positive(x: f64) -> f64 {
    const MOREBITS: f64 = 6.123_233_995_736_765_886_130e-17;
    const TAN3PIO8: f64 = 2.414_213_562_373_095_048_80;
    if x <= 0.66 {
        return atan_core(x);
    }
    if x > TAN3PIO8 {
        return FRAC_PI_2 - atan_core(1.0 / x) + MOREBITS;
    }
    FRAC_PI_4 + atan_core((x - 1.0) / (x + 1.0)) + 0.5 * MOREBITS
}

fn arctangent(x: f64) -> f64 {
    if x == 0.0 {
        return x;
    }
    if x > 0.0 {
        return atan_positive(x);
    }
    -atan_positive(-x)
}

fn arcsine(x: f64) -> f64 {
    if x == 0.0 {
        return x;
    }
    let mut x = x;
    let mut sign = false;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    if x > 1.0 {
        return nan();
    }
    let mut temp = (1.0 - x * x).sqrt();
    if x > 0.7 {
        temp = FRAC_PI_2 - atan_positive(temp / x);
    } else {
        temp = atan_positive(x / temp);
    }
    if sign { -temp } else { temp }
}

fn arccosine(x: f64) -> f64 {
    FRAC_PI_2 - arcsine(x)
}

fn arctangent2(y: f64, x: f64) -> f64 {
    /// 3 pi / 4, rounded once.
    const THREE_QUARTERS_PI: f64 = 2.356_194_490_192_345;
    if y.is_nan() || x.is_nan() {
        return nan();
    }
    if y == 0.0 {
        if x >= 0.0 && !x.is_sign_negative() {
            return 0f64.copysign(y);
        }
        return PI.copysign(y);
    }
    if x == 0.0 {
        return FRAC_PI_2.copysign(y);
    }
    if x.is_infinite() {
        if x > 0.0 {
            return if y.is_infinite() {
                FRAC_PI_4.copysign(y)
            } else {
                0f64.copysign(y)
            };
        }
        return if y.is_infinite() {
            THREE_QUARTERS_PI.copysign(y)
        } else {
            PI.copysign(y)
        };
    }
    if y.is_infinite() {
        return FRAC_PI_2.copysign(y);
    }
    let q = arctangent(y / x);
    if x < 0.0 {
        if q <= 0.0 {
            return q + PI;
        }
        return q - PI;
    }
    q
}

fn hyperbolic_sine(x: f64) -> f64 {
    const P0: f64 = -0.630_767_364_049_771_699_118_478_725_1e+6;
    const P1: f64 = -0.899_127_202_203_950_935_539_801_351_1e+5;
    const P2: f64 = -0.289_421_135_598_956_380_728_466_036_6e+4;
    const P3: f64 = -0.263_056_321_339_749_706_281_948_9e+2;
    const Q0: f64 = -0.630_767_364_049_771_699_121_207_727_7e+6;
    const Q1: f64 = 0.152_151_737_879_001_907_069_648_517_6e+5;
    const Q2: f64 = -0.173_678_953_558_233_699_533_450_911e+3;
    let mut x = x;
    let mut sign = false;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    let temp = if x > 21.0 {
        gomath::exp(x) * 0.5
    } else if x > 0.5 {
        let ex = gomath::exp(x);
        (ex - 1.0 / ex) * 0.5
    } else {
        let sq = x * x;
        let t = (((P3 * sq + P2) * sq + P1) * sq + P0) * x;
        t / (((sq + Q2) * sq + Q1) * sq + Q0)
    };
    if sign { -temp } else { temp }
}

fn hyperbolic_cosine(x: f64) -> f64 {
    let x = x.abs();
    if x > 21.0 {
        return gomath::exp(x) * 0.5;
    }
    let ex = gomath::exp(x);
    (ex + 1.0 / ex) * 0.5
}

fn hyperbolic_tangent(x: f64) -> f64 {
    const P: [f64; 3] = [
        -9.643_991_794_250_522_386_28e-1,
        -9.928_772_310_019_185_865_64e1,
        -1.614_687_684_417_084_479_52e3,
    ];
    const Q: [f64; 3] = [
        1.128_116_784_916_329_314_02e2,
        2.235_488_390_601_004_485_83e3,
        4.844_063_053_251_254_860_48e3,
    ];
    const MAXLOG: f64 = 8.802_969_193_111_305_429_598_8e+01;
    let mut z = x.abs();
    if z > 0.5 * MAXLOG {
        return if x < 0.0 { -1.0 } else { 1.0 };
    }
    if z >= 0.625 {
        let s = gomath::exp(2.0 * z);
        z = 1.0 - 2.0 / (s + 1.0);
        if x < 0.0 {
            z = -z;
        }
        return z;
    }
    if x == 0.0 {
        return x;
    }
    let s = x * x;
    x + x * s * ((P[0] * s + P[1]) * s + P[2]) / (((s + Q[0]) * s + Q[1]) * s + Q[2])
}

fn cube_root(x: f64) -> f64 {
    const B1: u64 = 715_094_163;
    const B2: u64 = 696_219_795;
    const C: f64 = 5.428_571_428_571_428_159_06e-01;
    const D: f64 = -7.053_061_224_489_796_110_50e-01;
    const E: f64 = 1.414_285_714_285_714_368_19e+00;
    const F: f64 = 1.607_142_857_142_857_206_30e+00;
    const G: f64 = 3.571_428_571_428_571_507_87e-01;
    if x == 0.0 || x.is_nan() || x.is_infinite() {
        return x;
    }
    let mut x = x;
    let mut sign = false;
    if x < 0.0 {
        x = -x;
        sign = true;
    }
    let mut t = f64::from_bits(x.to_bits() / 3 + (B1 << 32));
    if x < SMALLEST_NORMAL {
        t = (1u64 << 54) as f64;
        t *= x;
        t = f64::from_bits(t.to_bits() / 3 + (B2 << 32));
    }
    let mut r = t * t / x;
    let mut s = C + r * t;
    t *= G + F / (s + E + D / s);
    t = f64::from_bits((t.to_bits() & (0xF_FFFF_FFFC << 28)) + (1 << 30));
    s = t * t;
    r = x / s;
    let w = t + t;
    r = (r - t) / (w + r);
    t += t * r;
    if sign { -t } else { t }
}

#[cfg(test)]
mod tests {
    use std::collections::HashMap;

    use super::*;

    #[test]
    fn well_known_values() {
        assert_eq!(sine(1.0), 0.841_470_984_807_896_5);
        assert_eq!(cosine(1.0), 0.540_302_305_868_139_8);
        assert_eq!(gomath::exp(1.0), std::f64::consts::E);
        assert_eq!(gomath::log(2.0), std::f64::consts::LN_2);
        assert_eq!(gomath::pow(2.0, 10.0), 1024.0);
        assert_eq!(gomath::pow(10.0, -2.0), 0.01);
        assert_eq!(arctangent2(1.0, 1.0), FRAC_PI_4);
        assert_eq!(cube_root(27.0), 3.0);
        assert_eq!(log_base2(8.0), 3.0);
        assert_eq!(round_half_away(-2.5), -3.0);
        assert_eq!(round_half_away(0.49999999999999994), 0.0);
        assert!(gomath::log(-1.0).is_nan() && gomath::log(0.0) == f64::NEG_INFINITY);
    }

    fn same(a: f64, b: f64) -> bool {
        a.to_bits() == b.to_bits() || a.is_nan() && b.is_nan()
    }

    /// What the Go library gave for a sweep of arguments: `UDDIN_SWEEP_FILE` is the file that the test
    /// `TestMathSweep` of the oracle writes. The results must match to the last bit; a NaN matches a NaN.
    #[test]
    #[ignore = "reads the results of the Go library from the file UDDIN_SWEEP_FILE"]
    #[allow(
        clippy::disallowed_methods,
        reason = "the test reads a file that the Go library wrote"
    )]
    fn the_functions_match_the_go_library_to_the_last_bit() {
        let path = std::env::var("UDDIN_SWEEP_FILE").expect("UDDIN_SWEEP_FILE names the sweep");
        let data = std::fs::read(path).expect("the sweep file");
        let functions: HashMap<&str, fn(&[f64]) -> f64> = HashMap::from([
            ("sin", (|a| sine(a[0])) as fn(&[f64]) -> f64),
            ("cos", |a| cosine(a[0])),
            ("tan", |a| tangent(a[0])),
            ("asin", |a| arcsine(a[0])),
            ("acos", |a| arccosine(a[0])),
            ("atan", |a| arctangent(a[0])),
            ("sinh", |a| hyperbolic_sine(a[0])),
            ("cosh", |a| hyperbolic_cosine(a[0])),
            ("tanh", |a| hyperbolic_tangent(a[0])),
            ("cbrt", |a| cube_root(a[0])),
            ("exp", |a| gomath::exp(a[0])),
            ("exp2", |a| exponential2(a[0])),
            ("log", |a| gomath::log(a[0])),
            ("log10", |a| log_base10(a[0])),
            ("log2", |a| log_base2(a[0])),
            ("sqrt", |a| a[0].sqrt()),
            ("floor", |a| a[0].floor()),
            ("ceil", |a| a[0].ceil()),
            ("trunc", |a| a[0].trunc()),
            ("round", |a| round_half_away(a[0])),
            ("atan2", |a| arctangent2(a[0], a[1])),
            ("pow", |a| gomath::pow(a[0], a[1])),
            ("logb", |a| gomath::log(a[0]) / gomath::log(a[1])),
        ]);
        let line = |at: &mut usize| {
            let end = data[*at..]
                .iter()
                .position(|&b| b == b'\n')
                .expect("a line")
                + *at;
            let text = String::from_utf8_lossy(&data[*at..end]).into_owned();
            *at = end + 1;
            text
        };
        let mut at = 0;
        println!("{}", line(&mut at));
        let mut failures = Vec::new();
        let mut seen = 0;
        while at < data.len() {
            let header = line(&mut at);
            let mut words = header.split(' ');
            let name = words.next().expect("a name");
            let arity: usize = words.next().expect("an arity").parse().expect("a number");
            let count: usize = words.next().expect("a count").parse().expect("a number");
            let f = functions[name];
            let mut differ = 0;
            for _ in 0..count {
                let mut record = [0f64; 3];
                for slot in record.iter_mut().take(arity + 1) {
                    let bytes: [u8; 8] = data[at..at + 8].try_into().expect("eight bytes");
                    *slot = f64::from_bits(u64::from_le_bytes(bytes));
                    at += 8;
                }
                let got = f(&record[..arity]);
                if !same(got, record[arity]) {
                    differ += 1;
                    if differ <= 5 {
                        failures.push(format!(
                            "{name}({:?}): want {:#018x} got {:#018x}",
                            &record[..arity],
                            record[arity].to_bits(),
                            got.to_bits()
                        ));
                    }
                }
            }
            println!("{name}: {count} arguments, {differ} differ");
            seen += 1;
        }
        assert_eq!(seen, functions.len());
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}
