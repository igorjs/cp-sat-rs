// Copyright 2010-2025 Google LLC
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// Ported from ortools/util/full_precision_inequalities.h at commit c931f5f7. Changed: translated from C++ to Rust.
// Ported from ortools/util/full_precision_inequalities.cc at commit c931f5f7. Changed: translated from C++ to Rust.

use std::cmp::Ordering;

const K_INF: f64 = f64::INFINITY;
const K_DOUBLE_MIN: f64 = f64::from_bits(1);
const K_DOUBLE_MAX: f64 = f64::MAX;

// Function that is equivalent to std::nextafter(v, kInf), but significantly
// faster since it doesn't need to set or check CPU flags.
pub fn fast_step_up(v: f64) -> f64 {
    debug_assert!(!v.is_nan());
    const K_INF_BITS: i64 = f64::INFINITY.to_bits() as i64;
    const K_NEG_ZERO_BITS: i64 = (-0.0_f64).to_bits() as i64;
    let mut i = v.to_bits() as i64;
    if i == K_NEG_ZERO_BITS {
        i = 0;
    }
    i += if i != K_INF_BITS {
        if i >= 0 { 1 } else { -1 }
    } else {
        0
    };
    f64::from_bits(i as u64)
}

// Function that is equivalent to std::nextafter(v, -kInf), but significantly
// faster since it doesn't need to set or check CPU flags.
pub fn fast_step_down(_v: f64) -> f64 {
    unimplemented!("phase 4: util/full_precision_inequalities.cc")
}

// Returns `e` such that `2^e <= |v| < 2^(e + 1)`.
// For v = 0, returns `min_int`. For inf, returns `max_int`.
pub fn fast_ilog_b(_v: f64) -> i32 {
    unimplemented!("phase 4: util/full_precision_inequalities.cc")
}

// Returns `2^exponent` if representable, otherwise 0 or infinity (depending on
// the sign of the exponent).
pub fn fast_one_exp(_exponent: i32) -> f64 {
    unimplemented!("phase 4: util/full_precision_inequalities.cc")
}

// Returns the closest double to `v * 2^exponent` (infinity if overflow).
// It is truncated if the result is subnormal.
pub fn fast_ld_exp(_v: f64, _exponent: i32) -> f64 {
    unimplemented!("phase 4: util/full_precision_inequalities.cc")
}

// Represents a power of two, i.e. 2^exponent. Used for the API of TightSum.
// Supports between 2^-15 and 2^15 (the reason is to avoid overflows in the
// exponent).
#[derive(Clone, Copy, Debug)]
pub struct PowerOfTwo {
    pub exponent: i32,
}

impl PowerOfTwo {
    pub fn new(_exponent: i32) -> PowerOfTwo {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }
}

// Represents a sum of terms with full precision. The sum is not necessarily
// representable as a double. This is not thread-safe, but is thread-compatible.
//
// * The amortized complexity of adding a term is:
//  O(1 + exp_spread / num_terms)
//  where exp_spread = log(abs(max_nonzero_term)) - log(abs(min_nonzero_term)).
// * The complexity of getting the bounds is O(1).
// So if one adds N terms and gets the bounds no more than O(N) times, the
// complexity is O(N + exp_spread).
//
// Phase 0 ports what `sat/cp_model_utils` calls: adding a `f64` or an `i64`, the product of
// two of them, and reading the tight bounds. The rest is a stub that the LP and cuts phase
// fills in. The bounds are read through `&mut self`, since the C++ methods are `const` over
// `mutable` fields.
#[derive(Clone, Debug)]
pub struct TightSum {
    // The three fields below are used to store the sum in base 2^64. We use two
    // complement encoding for negative numbers. More precisely, the sum is equal
    // to:
    //   sum_(i = 0..digits_.size() - 1) digits_[i] * 2^(exponent_ + 64 * i)
    //       - negative_ * 2^(exponent_ + 64 * digits_.size())
    negative: bool,
    exponent: i32,
    digits: Vec<u64>,

    // If the sum is infinite, this is the sign. Once the sum is infinite, it
    // cannot change (it crashes if signs are not consistent). equal means that
    // the sum is finite.
    inf_sign: Ordering,

    // Known bounds on the sum.
    lb: f64,
    ub: f64,

    floor: Option<i64>,
    ceil: Option<i64>,

    // Whether the bounds are tight.
    bounds_updated: bool,
}

// Internal representation of a number.
// Note that a term needs not to be canonical: it is valid to represent 4.0 as
// 1.0 * 2^2 or 2.0 * 2^1 or 4.0 * 2^0.
#[derive(Clone, Copy, Debug)]
pub struct Term {
    mantissa: i128,
    exponent: i32,
}

// The types a `TightSum` accepts as arguments: the overloads of `ToTerm` and the lambdas
// of `SetInfIfNeeded`.
pub trait TightSumArg: Copy {
    fn to_term(self) -> Term;
    fn is_nan(self) -> bool;
    fn is_finite(self) -> bool;
    // Whether the argument is negative. Dies for 0, since `inf * 0` would give NaN.
    fn is_negative(self) -> bool;
}

impl TightSumArg for f64 {
    fn to_term(self) -> Term {
        debug_assert!(!self.is_nan());
        let bits = self.to_bits() as i64;
        let negative = bits < 0;
        let mut biased_exp = ((bits >> 52) & 0x7ff) as i32;
        let mut mantissa = bits & ((1_i64 << 52) - 1);
        if biased_exp > 0 {
            mantissa |= 1_i64 << 52;
        } else {
            biased_exp = 1;
        }
        Term {
            mantissa: i128::from(if negative { -mantissa } else { mantissa }),
            exponent: biased_exp - 1075,
        }
    }

    fn is_nan(self) -> bool {
        f64::is_nan(self)
    }

    fn is_finite(self) -> bool {
        f64::is_finite(self)
    }

    fn is_negative(self) -> bool {
        assert_ne!(self, 0.0, "inf * 0 would give NaN");
        self < 0.0
    }
}

impl TightSumArg for f32 {
    fn to_term(self) -> Term {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    fn is_nan(self) -> bool {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    fn is_finite(self) -> bool {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    fn is_negative(self) -> bool {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }
}

impl TightSumArg for i64 {
    fn to_term(self) -> Term {
        Term {
            mantissa: i128::from(self),
            exponent: 0,
        }
    }

    fn is_nan(self) -> bool {
        false
    }

    fn is_finite(self) -> bool {
        true
    }

    fn is_negative(self) -> bool {
        assert_ne!(self, 0, "inf * 0 would give NaN");
        self < 0
    }
}

impl TightSumArg for i32 {
    fn to_term(self) -> Term {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    fn is_nan(self) -> bool {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    fn is_finite(self) -> bool {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    fn is_negative(self) -> bool {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }
}

impl TightSumArg for PowerOfTwo {
    fn to_term(self) -> Term {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    fn is_nan(self) -> bool {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    fn is_finite(self) -> bool {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    fn is_negative(self) -> bool {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }
}

impl Default for TightSum {
    fn default() -> Self {
        TightSum {
            negative: false,
            exponent: 0,
            digits: Vec::new(),
            inf_sign: Ordering::Equal,
            lb: 0.0,
            ub: 0.0,
            floor: Some(0),
            ceil: Some(0),
            bounds_updated: true,
        }
    }
}

impl TightSum {
    pub fn clear(&mut self) {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    pub fn negate(&mut self) -> &mut Self {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    // Adds the given argument to the sum. Returns itself for chaining.
    pub fn add<Arg: TightSumArg>(&mut self, arg: Arg) -> &mut Self {
        self.set_inf_if_needed(arg);
        if self.inf_sign != Ordering::Equal {
            return self;
        }
        self.add_term(&arg.to_term());
        self
    }

    // `Add(const TightSum& other)`.
    pub fn add_tight_sum(&mut self, _other: &TightSum) -> &mut Self {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    // Adds the product of the given arguments to the sum. Returns itself for chaining.
    pub fn add_prod<Arg1: TightSumArg, Arg2: TightSumArg>(
        &mut self,
        arg1: Arg1,
        arg2: Arg2,
    ) -> &mut Self {
        self.set_inf_if_needed_prod(arg1, arg2);
        if self.inf_sign != Ordering::Equal {
            return self;
        }
        let mut term1 = arg1.to_term();
        // No overflow possible here.
        let term2 = arg2.to_term();
        term1.mantissa *= term2.mantissa;
        term1.exponent += term2.exponent;
        self.add_term(&term1);
        self
    }

    // The overload of `add_prod` for three or more arguments: the factors after the first two.
    pub fn add_prod_with_other_args<Arg1: TightSumArg, Arg2: TightSumArg>(
        &mut self,
        _arg1: Arg1,
        _arg2: Arg2,
        _other_args: &[Term],
    ) -> &mut Self {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    // The overload of `add_prod` for another `TightSum`.
    pub fn add_prod_tight_sum(
        &mut self,
        _other: &TightSum,
        _negate: bool,
        _scaling: PowerOfTwo,
    ) -> &mut Self {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    // Behind the comparison operators of C++ with a double: `sum <=> x`.
    pub fn compare_to_f64(&mut self, _x: f64) -> Ordering {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    // Behind the comparison operators of C++ with an int64: `sum <=> x`.
    pub fn compare_to_i64(&mut self, _x: i64) -> Ordering {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    // Returns true if no infinite terms have been added to the sum.
    pub fn is_finite(&self) -> bool {
        self.inf_sign == Ordering::Equal
    }

    // Deprecated upstream: use `tight_bounds`.
    pub fn loose_bounds(&mut self) -> (f64, f64) {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    // Deprecated upstream: use `tight_lower_bound`.
    pub fn loose_lower_bound(&mut self) -> f64 {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    // Deprecated upstream: use `tight_upper_bound`.
    pub fn loose_upper_bound(&mut self) -> f64 {
        unimplemented!("phase 4: util/full_precision_inequalities.cc")
    }

    // Returns the tight bounds of the sum. In other words, returns [lb, ub]
    // such that lb <= sum <= ub and
    //  * if sum is representable as a double, then lb == ub == sum;
    //  * otherwise, ub = nextafter(lb, inf).
    pub fn tight_bounds(&mut self) -> (f64, f64) {
        if !self.bounds_updated {
            self.update_bounds();
        }

        (self.lb, self.ub)
    }

    pub fn tight_lower_bound(&mut self) -> f64 {
        self.tight_bounds().0
    }

    pub fn tight_upper_bound(&mut self) -> f64 {
        self.tight_bounds().1
    }

    // Returns {floor(sum), ceil(sum)}.
    // * If sum > std::numeric_limits<int64_t>::max(), returns
    //   {std::numeric_limits<int64_t>::max(), nullopt}.
    // * If sum < std::numeric_limits<int64_t>::min(), returns
    //   {nullopt, std::numeric_limits<int64_t>::min()}.
    pub fn int64_bounds(&mut self) -> (Option<i64>, Option<i64>) {
        if !self.bounds_updated {
            self.update_bounds();
        }

        (self.floor, self.ceil)
    }

    pub fn int64_floor(&mut self) -> Option<i64> {
        self.int64_bounds().0
    }

    pub fn int64_ceil(&mut self) -> Option<i64> {
        self.int64_bounds().1
    }
}

// Returns the sum of a vector of doubles or int64_t.
pub fn tight_sum_of<Number: TightSumArg>(_v: &[Number]) -> TightSum {
    unimplemented!("phase 4: util/full_precision_inequalities.cc")
}

// Returns the dot product of two vectors of double or int64_t.
pub fn tight_dot_product<Number1: TightSumArg, Number2: TightSumArg>(
    _v: &[Number1],
    _w: &[Number2],
) -> TightSum {
    unimplemented!("phase 4: util/full_precision_inequalities.cc")
}

// For any combination of int64_t, finite double and PowerOfTwo, ProdBounds(a,
// b, p) returns a tight pair of bounds [lb, ub] such that lb <= a * b * p <= ub
// in R. The four overloads of C++ are one function over the types of rule 17.
pub fn prod_bounds<A: TightSumArg, B: TightSumArg>(_a: A, _b: B, _p: PowerOfTwo) -> (f64, f64) {
    unimplemented!("phase 4: util/full_precision_inequalities.cc")
}

// For any combination of int64_t, finite double and PowerOfTwo,
// ProdFloorCeil(a, b, p) returns Floor(a * b * p) and Ceil(a * b * p) as
// int64_t. If the result is not representable as int64_t, returns {nullopt,
// int64min} or {int64max, nullopt} depending on the sign.
pub fn prod_floor_ceil<A: TightSumArg, B: TightSumArg>(
    _a: A,
    _b: B,
    _p: PowerOfTwo,
) -> (Option<i64>, Option<i64>) {
    unimplemented!("phase 4: util/full_precision_inequalities.cc")
}

// Returns a tight double interval containing n.
pub fn int64_double_bounds(_n: i64) -> (f64, f64) {
    unimplemented!("phase 4: util/full_precision_inequalities.cc")
}

// Tight bounds [lb, ub] containing the exact rational quotient `a / b` for a
// finite `a` and a finite `b != 0` (`{q, q}` when `q = a / b` is exact, and a
// 1-ulp interval otherwise). A quotient that is too large to be representable
// gives `{max_double, inf}` (or `{-inf, -max_double}`).
pub fn quotient_bounds(_a: f64, _b: f64) -> (f64, f64) {
    unimplemented!("phase 4: util/full_precision_inequalities.cc")
}

impl TightSum {
    // Replaces TightSum by infinity if the incoming product is infinite. Crashes
    // if it would be NaN.
    fn set_inf_if_needed<Arg1: TightSumArg>(&mut self, arg1: Arg1) {
        assert!(!arg1.is_nan(), "NaN not supported");
        if arg1.is_finite() {
            // No factor is infinite so the sum does not change.
            return;
        }
        self.set_inf(arg1.is_negative());
    }

    // The overload of `set_inf_if_needed` for the product of two arguments.
    fn set_inf_if_needed_prod<Arg1: TightSumArg, Arg2: TightSumArg>(
        &mut self,
        arg1: Arg1,
        arg2: Arg2,
    ) {
        assert!(!arg1.is_nan() && !arg2.is_nan(), "NaN not supported");
        if arg1.is_finite() && arg2.is_finite() {
            // No factor is infinite so the sum does not change.
            return;
        }
        self.set_inf(arg1.is_negative() ^ arg2.is_negative());
    }

    fn set_inf(&mut self, prod_neg: bool) {
        let prod_sign = if prod_neg {
            Ordering::Less
        } else {
            Ordering::Greater
        };
        if self.inf_sign == Ordering::Equal {
            self.inf_sign = prod_sign;
            // Clear the sum to represent it as inf.
            self.digits.clear();
            // Fix once and for all the bounds.
            self.bounds_updated = true;
            if self.inf_sign == Ordering::Less {
                self.lb = -K_INF;
                self.ub = -K_INF;
                self.floor = None;
                self.ceil = Some(i64::MIN);
            } else {
                self.lb = K_INF;
                self.ub = K_INF;
                self.floor = Some(i64::MAX);
                self.ceil = None;
            }
        } else {
            assert_eq!(
                self.inf_sign, prod_sign,
                "Inconsistent infinite values in sum"
            );
        }
    }

    // Returns mantissa, exponent and exact such that
    //       mantissa * 2^exponent <= sum < (mantissa + 1) * 2^exponent and
    // mantissa < -2^62 or mantissa >= 2^62.
    // exact is true if and only if the left bound is an equality. The only
    // exception is when the sum is 0, where we return {0, 0, true}.
    fn extract_significand(&mut self) -> (i64, i32, bool) {
        // Recall that the sum is:
        //   sum_(i = 0..digits_.size() - 1) digits_[i] * 2^(exponent_ + 64 * i)
        //       - negative_ * 2^(exponent_ + 64 * digits_.size())
        // If the sum is negative, leading ones are insignificant while if the sum is
        // positive, leading zeros are insignificant.
        if self.negative {
            while self.digits.last() == Some(&u64::MAX) {
                self.digits.pop();
            }
        } else {
            while self.digits.last() == Some(&0) {
                self.digits.pop();
            }
        }
        let Some(&back) = self.digits.last() else {
            return if self.negative {
                (-1_i64 << 63, self.exponent - 63, true)
            } else {
                (0, 0, true)
            };
        };
        let size = self.digits.len();
        let sign: u64 = if self.negative { 1_u64 << 63 } else { 0 };
        let leading_insignificant = if self.negative {
            back.leading_ones()
        } else {
            back.leading_zeros()
        } as i32;

        if leading_insignificant <= 1 {
            // 63 significant bits are enough.
            let lost_digits = 1 - leading_insignificant;
            let lost_digits_mask = (1_u64 << lost_digits) - 1;
            let mantissa = (sign | (back >> lost_digits)) as i64;
            let exponent = self.exponent + 64 * (size as i32 - 1) + lost_digits;
            let exact = (back & lost_digits_mask) == 0
                && self.digits[..size - 1].iter().all(|&digit| digit == 0);
            return (mantissa, exponent, exact);
        }

        if size == 1 {
            let mantissa = (sign | (back << (leading_insignificant - 1))) as i64;
            let exponent = self.exponent + 1 - leading_insignificant;
            return (mantissa, exponent, true);
        }

        let lost_digits = 65 - leading_insignificant;
        let lost_digits_mask = (1_u64 << lost_digits) - 1;
        let comp2mantissa =
            (back << (leading_insignificant - 1)) | (self.digits[size - 2] >> lost_digits);
        let mantissa = (sign | comp2mantissa) as i64;
        let exponent = self.exponent + 64 * (size as i32 - 2) + lost_digits;
        let exact = (self.digits[size - 2] & lost_digits_mask) == 0
            && self.digits[..size - 2].iter().all(|&digit| digit == 0);
        (mantissa, exponent, exact)
    }

    fn update_bounds(&mut self) {
        // Mantissa and exponent such that
        //       mantissa * 2^exponent <= sum < (mantissa + 1) * 2^exponent and
        // mantissa < -2^62 or mantissa >= 2^62.
        // exact is true if and only if the left bound is an equality.
        self.bounds_updated = true;
        let (mantissa, exponent, exact) = self.extract_significand();
        (self.lb, self.ub) = to_double_bounds(mantissa, exponent, exact);
        (self.floor, self.ceil) = to_floor_ceil(mantissa, exponent, exact);
    }

    // Adds the given term to the sum.
    fn add_term(&mut self, term: &Term) {
        if term.mantissa == 0 {
            return;
        }

        self.bounds_updated = false;
        if self.digits.is_empty() && !self.negative {
            // Adds four digits_ to avoid resizing.
            self.digits.resize(6, 0);
            self.exponent = term.exponent - 4 * 64;
            // Note that we use two's complement for negative numbers so that we can
            // transfer directly from int128.
            self.digits[4] = term.mantissa as u64;
            self.digits[5] = (term.mantissa >> 64) as u64;
            self.negative = term.mantissa < 0;
            return;
        }

        // Decompose term.exponent = 64 * hi_exp + lo_exp + exponent_. (Do not use
        // div and mod as we do not want the usual C++ rounding, but lo_exp in [0,
        // 63]).
        let shifted_exp = term.exponent - self.exponent;
        let lo_exp = shifted_exp & 63;
        // Actual floor of the division by 64.
        let mut hi_exp = shifted_exp >> 6;
        let first_digit = (term.mantissa as u64) << lo_exp;
        let mut next_digits: i128 = term.mantissa >> (64 - lo_exp);
        if hi_exp < 0 {
            // Add the necessary number of digits + 4 to avoid making this too often.
            let num_digits_to_add = 4 - hi_exp;
            self.digits
                .splice(0..0, std::iter::repeat_n(0, num_digits_to_add as usize));
            hi_exp = 4;
            self.exponent -= num_digits_to_add << 6;
        }
        if hi_exp as usize + 1 >= self.digits.len() {
            let fill = if self.negative { u64::MAX } else { 0 };
            self.digits.resize(hi_exp as usize + 2, fill);
        }

        let data = &mut self.digits;
        let mut hi = hi_exp as usize;

        data[hi] = data[hi].wrapping_add(first_digit);
        if data[hi] < first_digit {
            next_digits += 1;
        }
        if next_digits == 0 {
            return;
        }

        hi += 1;
        let second_digit = next_digits as u64;
        let mut rem = (next_digits >> 64) as i64;
        data[hi] = data[hi].wrapping_add(second_digit);
        if data[hi] < second_digit {
            rem += 1;
        }
        if rem == 0 {
            return;
        }

        hi += 1;
        let size = data.len();
        let neg_added = term.mantissa < 0;
        if hi < size {
            data[hi] = data[hi].wrapping_add(rem as u64);
            if neg_added {
                if data[hi] < rem as u64 {
                    return;
                }

                rem = -1;
                hi += 1;
                while hi < size {
                    data[hi] = data[hi].wrapping_sub(1);
                    if data[hi] != u64::MAX {
                        return;
                    }
                    hi += 1;
                }
            } else {
                if data[hi] >= rem as u64 {
                    return;
                }

                rem = 1;
                hi += 1;
                while hi < size {
                    data[hi] = data[hi].wrapping_add(1);
                    if data[hi] > 0 {
                        return;
                    }
                    hi += 1;
                }
            }
        }
        // Now, we are at the end of digits_ with a non-zero carry.
        if self.negative {
            // Takes into account the term -2^(64 * digits_.size()).
            rem -= 1;
        }
        if rem != 0 {
            self.digits.push(rem as u64);
        }
        self.negative = neg_added;
    }
}

fn reconstruct_double(mantissa: u64, biased_exp: u64) -> f64 {
    f64::from_bits((biased_exp << 52) | (mantissa & !(1_u64 << 52)))
}

// Returns the double bounds of a real number with the given mantissa and
// exponent. Assumes that
//       mantissa * 2^exponent <= N < (mantissa + 1) * 2^exponent and
// mantissa < -2^62 or mantissa >= 2^62 or mantissa == 0. exact is true if and
// only if the left bound is an equality. This is the output contract of
// ExtractSignificand.
fn to_double_bounds(mantissa: i64, exponent: i32, exact: bool) -> (f64, f64) {
    if mantissa == 0 {
        return (0.0, 0.0);
    }

    debug_assert!(!(-1_i64 << 62..1_i64 << 62).contains(&mantissa));

    // Compute abs_mantissa and abs_exponent such that
    //       abs_mantissa * 2^abs_exponent
    //    <= abs(represented_number)
    //    <  (abs_mantissa + 1) * 2^abs_exponent and
    // 2^62 <= abs_mantissa < 2^63.
    let abs_mantissa: u64;
    let mut abs_exponent = exponent;
    if mantissa >= 0 {
        abs_mantissa = mantissa as u64;
    } else if exact {
        if mantissa == i64::MIN {
            abs_mantissa = 1_u64 << 62;
            abs_exponent += 1;
        } else {
            abs_mantissa = (-mantissa) as u64;
        }
    } else {
        abs_mantissa = !(mantissa as u64);
    }

    let binary_exp = abs_exponent + 62;
    if binary_exp > 1023 {
        return if mantissa < 0 {
            (-K_INF, -K_DOUBLE_MAX)
        } else {
            (K_DOUBLE_MAX, K_INF)
        };
    }
    if binary_exp < -1074 {
        return if mantissa < 0 {
            (-K_DOUBLE_MIN, 0.0)
        } else {
            (0.0, K_DOUBLE_MIN)
        };
    }
    let sig_bits = 53.min(binary_exp + 1075);
    let lost_bits = 63 - sig_bits;
    let dbl_mantissa = abs_mantissa >> lost_bits;
    let biased_exponent = (binary_exp + 1023).max(0) as u64;
    let dbl_exact = exact && (abs_mantissa & ((1_u64 << lost_bits) - 1)) == 0;
    let mag_lb = reconstruct_double(dbl_mantissa, biased_exponent);
    let mag_ub = if dbl_exact {
        mag_lb
    } else {
        fast_step_up(mag_lb)
    };
    if mantissa < 0 {
        (-mag_ub, -mag_lb)
    } else {
        (mag_lb, mag_ub)
    }
}

// Returns the floor and ceil of a real number with the given mantissa and
// exponent. Assumes that
//       mantissa * 2^exponent <= N < (mantissa + 1) * 2^exponent and
// mantissa < -2^62 or mantissa >= 2^62 or mantissa == 0. exact is true if and
// only if the left bound is an equality. This is the output contract of
// ExtractSignificand.
fn to_floor_ceil(mantissa: i64, exponent: i32, exact: bool) -> (Option<i64>, Option<i64>) {
    debug_assert!(mantissa == 0 || !(-1_i64 << 62..1_i64 << 62).contains(&mantissa));
    if exponent > 0 {
        return if mantissa < 0 {
            (None, Some(i64::MIN))
        } else {
            (Some(i64::MAX), None)
        };
    }
    if exponent <= -64 {
        return if mantissa < 0 {
            (Some(-1), Some(0))
        } else {
            (Some(0), Some(1))
        };
    }
    let lost_digits = -exponent;
    let fl = mantissa >> lost_digits;
    let int_exact = exact && (mantissa as u64 & ((1_u64 << lost_digits) - 1)) == 0;
    (
        Some(fl),
        if int_exact {
            Some(fl)
        } else if fl == i64::MAX {
            None
        } else {
            Some(fl + 1)
        },
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    // PENDING(phase 4): TEST_P(ComputeSumSignTest, Pos), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ComputeSumSignTest, Neg), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(IsDotProductCmpOrEqualTest, PosPos), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(IsDotProductCmpOrEqualTest, PosNeg), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(IsDotProductCmpOrEqualTest, NegPos), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(IsDotProductCmpOrEqualTest, NegNeg), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(IsDotProductCmpOrEqualTest, VeryLongSum), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(GetTightDotProductBoundsTest, PosPos), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(GetTightDotProductBoundsTest, PosNeg), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(GetTightDotProductBoundsTest, NegPos), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(GetTightDotProductBoundsTest, NegNeg), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ProdBoundsIntIntTest, PosPos), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ProdBoundsIntIntTest, PosNeg), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ProdBoundsIntIntTest, NegPos), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ProdBoundsIntIntTest, NegNeg), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ProdBoundsIntDoubleTest, PosPos), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ProdBoundsIntDoubleTest, PosNeg), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ProdBoundsIntDoubleTest, NegPos), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ProdBoundsIntDoubleTest, NegNeg), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ProdBoundsDoubleDoubleTest, PosPos), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ProdBoundsDoubleDoubleTest, PosNeg), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ProdBoundsDoubleDoubleTest, NegPos), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ProdBoundsDoubleDoubleTest, NegNeg), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ProdFloorCeilIntIntTest, FloorAndCeilMatchExpected), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ProdFloorCeilIntDoubleTest, FloorAndCeilMatchExpected), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(ProdFloorCeilDoubleDoubleTest, FloorAndCeilMatchExpected), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(Int64DoubleBoundsTest, Pos), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(Int64DoubleBoundsTest, Neg), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(QuotientBoundsTest, PosPos), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(QuotientBoundsTest, PosNeg), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(QuotientBoundsTest, NegPos), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(QuotientBoundsTest, NegNeg), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(TightSumTest, DefaultConstructedIsZero), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(TightSumTest, AddProdSingleArgumentTypesAndChaining), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(TightSumTest, AddProdTwoArgumentCombinations), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(TightSumTest, AddProdThreeOrMoreArgumentsWithoutOverflow), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(TightSumTest, AddProdThreeOrMoreArgumentsWithMantissaOverflow), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(TightSumTest, AddProdTenArgumentsWithDelayedMantissaOverflow), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(TightSumComparisonTest, LValueAndRValueComparisons), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(TightSumComparisonTest, NegatedComparisons), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(TightSumInt64ComparisonTest, LValueAndRValueComparisons), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(TightSumInt64ComparisonTest, NegatedComparisons), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(TightSumTightBoundsTest, BoundsAndSpecificationInvariants), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(TightSumTightBoundsTest, NegatedBounds), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(TightSumTest, UnderflowSubnormalBounds), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(TightSumTest, Int64ExtremeValues), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(TightSumTest, AccumulatorUint128Overflow), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(TightSumTest, IncrementalAdditionsAndQueries), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(TightSumTest, InfiniteProductsAndConsistentSigns), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(TightSumTest, TightSumOfAndTightDotProductHelpers), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(TightSumDeathTest, InvalidInputsCrash), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST_P(TightSumFloorCeilTest, FloorAndCeilMatchExpected), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(TightSumTest, SaturatedFloorAndCeilUnderflowSubnormals), needs util/full_precision_inequalities.cc

    #[test]
    fn tight_sum_test_infinite_works() {
        // Arrange
        const K_INF: f64 = f64::INFINITY;

        // Act and Assert
        assert_eq!(
            TightSum::default().add(K_INF).tight_bounds(),
            (K_INF, K_INF)
        );
        assert_eq!(
            TightSum::default().add(-K_INF).tight_bounds(),
            (-K_INF, -K_INF)
        );
    }

    // PENDING(phase 4): TEST(TightSumTest, AddAndAddProdWithTightSum), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(TightSumTest, Negate), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(FastILogBTest, SpecialValues), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(FastILogBTest, NormalAndSubnormalValues), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(FastOneExpTest, NormalSubnormalAndOutOfRangeExponents), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(FastLdExpTest, ZeroAndInfinities), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(FastLdExpTest, NormalInputsToNormalSubnormalUnderflowAndOverflow), needs util/full_precision_inequalities.cc
    // PENDING(phase 4): TEST(FastLdExpTest, SubnormalInputsToZeroSubnormalNormalAndOverflow), needs util/full_precision_inequalities.cc

    // The bounds of the next two tests were read from the reference build at the pinned commit.
    #[test]
    fn a_negative_sum_that_is_not_a_double_gets_one_step_wide_bounds() {
        // Arrange
        let mut sum = TightSum::default();

        // Act
        sum.add(-1.0_f64).add(-(2.0_f64.powi(-53)));

        // Assert
        assert_eq!(sum.tight_bounds(), (-(1.0_f64.next_up()), -1.0));
        assert_eq!(sum.int64_bounds(), (Some(-2), Some(-1)));
    }

    // The case `{1.0, -denorm_min}` of the parameters of `TightSumTightBoundsTest`.
    #[test]
    fn a_sum_just_under_a_double_gets_bounds_that_end_on_it() {
        // Arrange
        let mut sum = TightSum::default();

        // Act
        sum.add(1.0_f64).add(-f64::from_bits(1));

        // Assert
        assert_eq!(sum.tight_bounds(), (1.0_f64.next_down(), 1.0));
        assert_eq!(sum.int64_bounds(), (Some(0), Some(1)));
    }

    #[test]
    fn a_sum_that_is_not_a_double_gets_one_step_wide_bounds() {
        // Arrange
        let mut sum = TightSum::default();

        // Act
        sum.add(1.0_f64).add(2.0_f64.powi(-53));

        // Assert
        assert_eq!(sum.tight_bounds(), (1.0, 1.0_f64.next_up()));
        assert_eq!(sum.int64_bounds(), (Some(1), Some(2)));
    }

    #[test]
    fn a_negative_exact_sum_has_equal_bounds() {
        // Arrange
        let mut sum = TightSum::default();

        // Act
        sum.add(-0.5_f64).add(-0.25_f64);

        // Assert
        assert_eq!(sum.tight_bounds(), (-0.75, -0.75));
        assert_eq!(sum.int64_bounds(), (Some(-1), Some(0)));
    }

    #[test]
    fn terms_that_cancel_leave_the_small_term() {
        // Arrange
        let mut sum = TightSum::default();

        // Act
        sum.add(1e300_f64).add(1.0_f64).add(-1e300_f64);

        // Assert
        assert_eq!(sum.tight_bounds(), (1.0, 1.0));
        assert_eq!(sum.int64_bounds(), (Some(1), Some(1)));
    }

    #[test]
    fn a_product_past_int64_has_no_upper_integer_bound() {
        // Arrange
        let mut sum = TightSum::default();

        // Act
        sum.add_prod(2.0_f64, i64::MAX);

        // Assert
        assert_eq!(sum.int64_bounds(), (Some(i64::MAX), None));
        assert_eq!(
            sum.tight_bounds(),
            (2.0_f64.powi(64).next_down(), 2.0_f64.powi(64))
        );
    }

    #[test]
    fn an_infinite_term_makes_the_sum_infinite_and_fixes_its_sign() {
        // Arrange
        let mut sum = TightSum::default();

        // Act
        sum.add(f64::INFINITY).add(1.0_f64);

        // Assert
        assert!(!sum.is_finite());
        assert_eq!(sum.tight_bounds(), (f64::INFINITY, f64::INFINITY));
        assert_eq!(sum.int64_bounds(), (Some(i64::MAX), None));
    }

    #[test]
    #[should_panic(expected = "Inconsistent infinite values in sum")]
    fn opposite_infinite_terms_die() {
        // Arrange
        let mut sum = TightSum::default();
        sum.add(f64::INFINITY);

        // Act
        sum.add(f64::NEG_INFINITY);
    }

    #[test]
    #[should_panic(expected = "inf * 0 would give NaN")]
    fn an_infinite_factor_times_zero_dies() {
        // Arrange
        let mut sum = TightSum::default();

        // Act
        sum.add_prod(f64::INFINITY, 0_i64);
    }
}
