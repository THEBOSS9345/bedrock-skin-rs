//! Go's `math.Sin`, `math.Cos`, `math.Tan`, `math.Atan`, `math.Asin`,
//! `math.Acos`, `math.Atan2`, `math.Min` and `math.Max`, ported line for line.
//!
//! The Go version of this library computes its camera and every animation
//! with these, and Rust's own `sin`/`cos`/`tan` call the platform's C library,
//! which can round the last bit differently. One bit is enough to move an
//! edge pixel, so this crate uses Go's implementations to render the same
//! images bit for bit. See docs/design-decisions.md#why-gos-trigonometry.

#![allow(clippy::excessive_precision)] // the constants are Go's, digit for digit

const PI4A: f64 = 7.853_981_256_484_985_351_56e-1; // 0x3fe921fb40000000, Pi/4 split into three parts
const PI4B: f64 = 3.774_894_707_930_798_176_68e-8; // 0x3e64442d00000000
const PI4C: f64 = 2.695_151_429_079_059_526_45e-15; // 0x3ce8469898cc5170

const FOUR_OVER_PI: f64 = 4.0 / std::f64::consts::PI; // 0x3ff45f306dc9c883, as Go folds 4/Pi

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

const TAN_P: [f64; 3] = [
    -1.309_369_391_813_837_776_46e4,
    1.153_516_648_385_874_161_40e6,
    -1.795_652_519_764_848_779_88e7,
];

const TAN_Q: [f64; 5] = [
    1.0,
    1.368_129_634_706_929_546_78e4,
    -1.320_892_344_402_109_674_47e6,
    2.500_838_018_233_579_158_39e7,
    -5.386_957_559_294_546_298_81e7,
];

const REDUCE_THRESHOLD: f64 = (1u64 << 29) as f64;

fn sin_poly(z: f64, zz: f64) -> f64 {
    z + z
        * zz
        * ((((((SIN[0] * zz) + SIN[1]) * zz + SIN[2]) * zz + SIN[3]) * zz + SIN[4]) * zz + SIN[5])
}

fn cos_poly(zz: f64) -> f64 {
    1.0 - 0.5 * zz
        + zz * zz
            * ((((((COS[0] * zz) + COS[1]) * zz + COS[2]) * zz + COS[3]) * zz + COS[4]) * zz
                + COS[5])
}

/// The octant and the angle within it, for x >= 0.
fn reduce(x: f64, wrap: bool) -> (u64, f64) {
    if x >= REDUCE_THRESHOLD {
        return trig_reduce(x);
    }
    let mut j = (x * FOUR_OVER_PI) as u64;
    let mut y = j as f64;
    if j & 1 == 1 {
        j += 1;
        y += 1.0;
    }
    if wrap {
        j &= 7;
    }
    (j, ((x - y * PI4A) - y * PI4B) - y * PI4C)
}

pub(crate) fn sin(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        return x;
    }
    if x.is_infinite() {
        return f64::NAN;
    }
    let (mut sign, x) = if x < 0.0 { (true, -x) } else { (false, x) };
    let (mut j, z) = reduce(x, true);
    if j > 3 {
        sign = !sign;
        j -= 4;
    }
    let zz = z * z;
    let y = if j == 1 || j == 2 {
        cos_poly(zz)
    } else {
        sin_poly(z, zz)
    };
    if sign { -y } else { y }
}

pub(crate) fn cos(x: f64) -> f64 {
    if x.is_nan() || x.is_infinite() {
        return f64::NAN;
    }
    let mut sign = false;
    let (mut j, z) = reduce(x.abs(), true);
    if j > 3 {
        j -= 4;
        sign = !sign;
    }
    if j > 1 {
        sign = !sign;
    }
    let zz = z * z;
    let y = if j == 1 || j == 2 {
        sin_poly(z, zz)
    } else {
        cos_poly(zz)
    };
    if sign { -y } else { y }
}

pub(crate) fn tan(x: f64) -> f64 {
    if x == 0.0 || x.is_nan() {
        return x;
    }
    if x.is_infinite() {
        return f64::NAN;
    }
    let (sign, x) = if x < 0.0 { (true, -x) } else { (false, x) };
    let (j, z) = reduce(x, false);
    let zz = z * z;
    let mut y = if zz > 1e-14 {
        z + z
            * (zz * (((TAN_P[0] * zz) + TAN_P[1]) * zz + TAN_P[2])
                / ((((zz + TAN_Q[1]) * zz + TAN_Q[2]) * zz + TAN_Q[3]) * zz + TAN_Q[4]))
    } else {
        z
    };
    if j & 2 == 2 {
        y = -1.0 / y;
    }
    if sign { -y } else { y }
}

const M_PI4: [u64; 20] = [
    0x0000000000000001,
    0x45f306dc9c882a53,
    0xf84eafa3ea69bb81,
    0xb6c52b3278872083,
    0xfca2c757bd778ac3,
    0x6e48dc74849ba5c0,
    0x0c925dd413a32439,
    0xfc3bd63962534e7d,
    0xd1046bea5d768909,
    0xd338e04d68befc82,
    0x7323ac7306a673e9,
    0x3908bf177bf25076,
    0x3ff12fffbc0b301f,
    0xde5e2316b414da3e,
    0xda6cfd9e4f96136e,
    0x9e8c7ecd3cbfd45a,
    0xea4f758fd7cbe2f6,
    0x7a0e73ef14a525d4,
    0xd7f6bf623f1aba10,
    0xac06608df8f6d757,
];

/// Go's shifts: shifting a u64 by 64 or more gives 0.
fn shl(x: u64, n: u32) -> u64 {
    x.checked_shl(n).unwrap_or(0)
}

fn shr(x: u64, n: u32) -> u64 {
    x.checked_shr(n).unwrap_or(0)
}

/// Payne-Hanek range reduction by Pi/4 for huge x, as Go's trigReduce.
fn trig_reduce(x: f64) -> (u64, f64) {
    const PI4: f64 = std::f64::consts::PI / 4.0;
    const SHIFT: u32 = 52;
    const MASK: u64 = 0x7ff;
    const BIAS: i64 = 1023;
    if x < PI4 {
        return (0, x);
    }
    let mut ix = x.to_bits();
    let exp = ((ix >> SHIFT) & MASK) as i64 - BIAS - SHIFT as i64;
    ix &= !(MASK << SHIFT);
    ix |= 1 << SHIFT;
    let digit = ((exp + 61) / 64) as usize;
    let bitshift = ((exp + 61) % 64) as u32;
    let z0 = shl(M_PI4[digit], bitshift) | shr(M_PI4[digit + 1], 64 - bitshift);
    let z1 = shl(M_PI4[digit + 1], bitshift) | shr(M_PI4[digit + 2], 64 - bitshift);
    let z2 = shl(M_PI4[digit + 2], bitshift) | shr(M_PI4[digit + 3], 64 - bitshift);
    let z2hi = ((z2 as u128 * ix as u128) >> 64) as u64;
    let p1 = z1 as u128 * ix as u128;
    let (z1hi, z1lo) = ((p1 >> 64) as u64, p1 as u64);
    let z0lo = z0.wrapping_mul(ix);
    let (lo, carry) = z1lo.overflowing_add(z2hi);
    let mut hi = z0lo.wrapping_add(z1hi).wrapping_add(carry as u64);
    let mut j = hi >> 61;
    hi = (hi << 3) | (lo >> 61);
    let lz = hi.leading_zeros();
    let e = (BIAS as u64).wrapping_sub(lz as u64 + 1);
    hi = shl(hi, lz + 1) | shr(lo, 64 - (lz + 1));
    hi >>= 64 - SHIFT;
    hi |= e << SHIFT;
    let mut z = f64::from_bits(hi);
    if j & 1 == 1 {
        j += 1;
        j &= 7;
        z -= 1.0;
    }
    (j, z * PI4)
}

fn xatan(x: f64) -> f64 {
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

/// atan of a positive x, reduced to [0, 0.66].
fn satan(x: f64) -> f64 {
    const MOREBITS: f64 = 6.123_233_995_736_765_886_130e-17; // pi/2 = PIO2 + Morebits
    const TAN3PIO8: f64 = 2.414_213_562_373_095_048_80; // tan(3*pi/8)
    if x <= 0.66 {
        return xatan(x);
    }
    if x > TAN3PIO8 {
        return std::f64::consts::FRAC_PI_2 - xatan(1.0 / x) + MOREBITS;
    }
    std::f64::consts::FRAC_PI_4 + xatan((x - 1.0) / (x + 1.0)) + 0.5 * MOREBITS
}

pub(crate) fn atan(x: f64) -> f64 {
    if x == 0.0 {
        return x;
    }
    if x > 0.0 { satan(x) } else { -satan(-x) }
}

pub(crate) fn asin(x: f64) -> f64 {
    if x == 0.0 {
        return x;
    }
    let (sign, x) = if x < 0.0 { (true, -x) } else { (false, x) };
    if x > 1.0 {
        return f64::NAN;
    }
    let mut temp = (1.0 - x * x).sqrt();
    temp = if x > 0.7 {
        std::f64::consts::FRAC_PI_2 - satan(temp / x)
    } else {
        satan(x / temp)
    };
    if sign { -temp } else { temp }
}

pub(crate) fn acos(x: f64) -> f64 {
    std::f64::consts::FRAC_PI_2 - asin(x)
}

pub(crate) fn atan2(y: f64, x: f64) -> f64 {
    use std::f64::consts::{FRAC_PI_2, FRAC_PI_4, PI};
    if y.is_nan() || x.is_nan() {
        return f64::NAN;
    }
    if y == 0.0 {
        if x >= 0.0 && !x.is_sign_negative() {
            return 0.0f64.copysign(y);
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
                0.0f64.copysign(y)
            };
        }
        return if y.is_infinite() {
            (3.0 * PI / 4.0).copysign(y)
        } else {
            PI.copysign(y)
        };
    }
    if y.is_infinite() {
        return FRAC_PI_2.copysign(y);
    }
    let q = atan(y / x);
    if x < 0.0 {
        return if q <= 0.0 { q + PI } else { q - PI };
    }
    q
}

/// Go's math.Min: -Inf beats NaN, NaN beats everything else, and -0 is less
/// than +0. Rust's f64::min ignores a NaN instead.
pub(crate) fn min(x: f64, y: f64) -> f64 {
    if x == f64::NEG_INFINITY || y == f64::NEG_INFINITY {
        return f64::NEG_INFINITY;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 && x == y {
        return if x.is_sign_negative() { x } else { y };
    }
    if x < y { x } else { y }
}

/// Go's math.Max, the mirror of [`min`].
pub(crate) fn max(x: f64, y: f64) -> f64 {
    if x == f64::INFINITY || y == f64::INFINITY {
        return f64::INFINITY;
    }
    if x.is_nan() || y.is_nan() {
        return f64::NAN;
    }
    if x == 0.0 && x == y {
        return if x.is_sign_negative() { y } else { x };
    }
    if x > y { x } else { y }
}

/// Go's int(f) on amd64: the conversion instruction gives the minimum int64
/// for NaN and anything out of range, where Rust's `as` saturates.
pub(crate) fn go_int(f: f64) -> i64 {
    if f.is_nan() || f >= 9.223_372_036_854_775_807e18 || f < -9.223_372_036_854_775_808e18 {
        i64::MIN
    } else {
        f as i64
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// testdata/parity/trig.json holds what Go's math package returns, by
    /// bit pattern, for a spread of inputs (tools/parity writes it).
    #[test]
    fn matches_go() {
        let cases: serde_json::Value =
            serde_json::from_slice(&std::fs::read("testdata/parity/trig.json").unwrap()).unwrap();
        let bits = |v: &serde_json::Value| u64::from_str_radix(v.as_str().unwrap(), 16).unwrap();
        for c in cases.as_array().unwrap() {
            let x = c["X"].as_f64().unwrap();
            assert_eq!(sin(x).to_bits(), bits(&c["Sin"]), "sin({x})");
            assert_eq!(cos(x).to_bits(), bits(&c["Cos"]), "cos({x})");
            assert_eq!(tan(x).to_bits(), bits(&c["Tan"]), "tan({x})");
        }
    }

    #[test]
    fn min_max_like_go() {
        assert!(min(1.0, f64::NAN).is_nan());
        assert_eq!(min(f64::NAN, f64::NEG_INFINITY), f64::NEG_INFINITY);
        assert!(min(-0.0, 0.0).is_sign_negative());
        assert!(max(-0.0, 0.0).is_sign_positive());
        assert_eq!(go_int(f64::NAN), i64::MIN);
        assert_eq!(go_int(-2.7), -2);
    }
}
