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
// Ported from ortools/util/saturated_arithmetic.h at commit c931f5f7. Changed: translated from C++ to Rust.

//! Saturated addition, subtraction and multiplication on `i64`.
//!
//! There are two versions of the code. The built-in versions use the overflow checks of the
//! integer types. The generic versions (`cap_add_generic`, `cap_sub_generic`,
//! `cap_prod_generic`) use bit tricks only. `cap_add`, `cap_sub` and `cap_prod` call the
//! built-in versions, as upstream does under GCC and Clang.

use crate::bitset::most_significant_bit_position64;

/// A strong integer type over `i64`, as made by `DEFINE_STRONG_INT64_TYPE`.
pub trait StrongInt64: Copy {
    fn value(self) -> i64;
    fn from_value(value: i64) -> Self;
}

/// Checks if x is equal to the min or the max value of an int64_t.
pub fn at_min_or_max_int64(x: i64) -> bool {
    x == i64::MIN || x == i64::MAX
}

// NOTE(user): -kint64min != kint64max, but kint64max == ~kint64min.
pub fn cap_opp(v: i64) -> i64 {
    if v == i64::MIN { !v } else { -v }
}

pub fn cap_abs(v: i64) -> i64 {
    if v == i64::MIN {
        i64::MAX
    } else if v < 0 {
        -v
    } else {
        v
    }
}

// ---------- Overflow utility functions ----------

/// Implements two's complement addition on int64s.
pub fn twos_complement_addition(x: i64, y: i64) -> i64 {
    x.wrapping_add(y)
}

/// Implements two's complement subtraction on int64s.
pub fn twos_complement_subtraction(x: i64, y: i64) -> i64 {
    x.wrapping_sub(y)
}

/// Helper function that returns true if an overflow has occurred in computing
/// sum = x + y. sum is expected to be computed elsewhere.
pub fn add_had_overflow(x: i64, y: i64, sum: i64) -> bool {
    // Overflow cannot occur if operands have different signs.
    // It can only occur if sign(x) == sign(y) and sign(sum) != sign(x),
    // which is equivalent to: sign(x) != sign(sum) && sign(y) != sign(sum).
    // This is captured when the expression below is negative.
    debug_assert_eq!(sum, twos_complement_addition(x, y));
    ((x ^ sum) & (y ^ sum)) < 0
}

pub fn sub_had_overflow(x: i64, y: i64, diff: i64) -> bool {
    // This is the same reasoning as for add_had_overflow. We have x = diff + y.
    // The formula is the same, with 'x' and diff exchanged.
    debug_assert_eq!(diff, twos_complement_subtraction(x, y));
    add_had_overflow(diff, y, x)
}

// A note on overflow treatment.
// kint64min and kint64max are treated as infinity.
// Thus if the computation overflows, the result is always kint64m(ax/in).
//
// NOTE(user): this is actually wrong: when computing A-B, if A is kint64max
// and B is finite, then A-B won't be kint64max: overflows aren't sticky.
// TODO(user): consider making some operations overflow-sticky, some others
// not, but make an explicit choice throughout.
pub fn add_overflows(x: i64, y: i64) -> bool {
    add_had_overflow(x, y, twos_complement_addition(x, y))
}

// Upstream returns an int64_t here, not a bool.
pub fn sub_overflows(x: i64, y: i64) -> i64 {
    i64::from(sub_had_overflow(x, y, twos_complement_subtraction(x, y)))
}

/// Performs *b += a and returns false iff the addition overflow or underflow.
/// This function only works for typed integer type (IntType<>).
pub fn safe_add_into<T: StrongInt64>(a: T, b: &mut T) -> bool {
    let x = a.value();
    let y = b.value();
    let sum = twos_complement_addition(x, y);
    if add_had_overflow(x, y, sum) {
        return false;
    }
    *b = T::from_value(sum);
    true
}

/// Returns kint64max if x >= 0 and kint64min if x < 0.
pub fn cap_with_sign_of(x: i64) -> i64 {
    // return kint64max if x >= 0 or kint64max + 1 (== kint64min) if x < 0.
    twos_complement_addition(i64::MAX, i64::from(x < 0))
}

// Simple implementations using the overflow checks of the integer types, which compile well on
// both x86_64 and ARM.
pub fn cap_add_built_in(x: i64, y: i64) -> i64 {
    let cap = cap_with_sign_of(x);
    let (result, overflowed) = x.overflowing_add(y);
    if overflowed { cap } else { result }
}

pub fn cap_sub_built_in(x: i64, y: i64) -> i64 {
    let cap = cap_with_sign_of(x);
    let (result, overflowed) = x.overflowing_sub(y);
    if overflowed { cap } else { result }
}

pub fn cap_prod_built_in(x: i64, y: i64) -> i64 {
    let cap = cap_with_sign_of(x ^ y);
    let (result, overflowed) = x.overflowing_mul(y);
    if overflowed { cap } else { result }
}

// Generic implementations. They are very good for addition and subtraction,
// less so for multiplication.
pub fn cap_add_generic(x: i64, y: i64) -> i64 {
    let result = twos_complement_addition(x, y);
    if add_had_overflow(x, y, result) {
        cap_with_sign_of(x)
    } else {
        result
    }
}

pub fn cap_sub_generic(x: i64, y: i64) -> i64 {
    let result = twos_complement_subtraction(x, y);
    if sub_had_overflow(x, y, result) {
        cap_with_sign_of(x)
    } else {
        result
    }
}

pub mod cap_prod_util {
    /// Returns an unsigned int equal to the absolute value of n, in a way that
    /// will not produce overflows.
    pub fn uint_abs(n: i64) -> u64 {
        if n < 0 {
            (!(n as u64)).wrapping_add(1)
        } else {
            n as u64
        }
    }
}

/// The generic algorithm computes a bound on the number of bits necessary to
/// store the result. For this it uses the position of the most significant bits
/// of each of the arguments.
/// If the result needs at least 64 bits, then return a capped value.
/// If the result needs at most 63 bits, then return the product.
/// Otherwise, the result may use 63 or 64 bits: compute the product
/// as a uint64_t, and cap it if necessary.
pub fn cap_prod_generic(x: i64, y: i64) -> i64 {
    let a = cap_prod_util::uint_abs(x);
    let b = cap_prod_util::uint_abs(y);
    // Let MSB(x) denote the most significant bit of x. We have:
    // MSB(x) + MSB(y) <= MSB(x * y) <= MSB(x) + MSB(y) + 1
    let msb_sum = most_significant_bit_position64(a) + most_significant_bit_position64(b);
    const K_MAX_BIT_INDEX_IN_INT64: i32 = 63;
    if msb_sum <= K_MAX_BIT_INDEX_IN_INT64 - 2 {
        return x * y;
    }
    // Catch a == 0 or b == 0 now, as MostSignificantBitPosition64(0) == 0.
    // TODO(user): avoid this by writing function Log2(a) with Log2(0) == -1.
    if a == 0 || b == 0 {
        return 0;
    }
    let cap = cap_with_sign_of(x ^ y);
    if msb_sum >= K_MAX_BIT_INDEX_IN_INT64 {
        return cap;
    }
    // The corner case is when msb_sum == 62, i.e. at least 63 bits will be
    // needed to store the product. The following product will never overflow
    // on uint64_t, since msb_sum == 62.
    let u_prod = a * b;
    // The overflow cases are captured by one of the following conditions:
    // (cap >= 0 && u_prod >= static_cast<uint64_t>(kint64max) or
    // (cap < 0 && u_prod >= static_cast<uint64_t>(kint64min)).
    // These can be optimized as follows (and if the condition is false, it is
    // safe to compute x * y.
    if u_prod >= cap as u64 {
        return cap;
    }
    let abs_result = u_prod as i64;
    if cap < 0 { -abs_result } else { abs_result }
}

/// A generic, safer version of cap_add() where we don't silently convert double
/// or int32_t to int64_t. When the inputs are floating-point, uses '+', else
/// uses cap_add() for int64_t, and a (slow-ish) int32_t version for int.
pub trait CapOrFloatAdd: Sized {
    fn cap_or_float_add(x: Self, y: Self) -> Self;
}

pub fn cap_add(x: i64, y: i64) -> i64 {
    cap_add_built_in(x, y)
}

/// This avoid the need to convert to int64_t min/max and is about twice as fast
/// if it corresponds to your use case.
pub fn add_into_overflow(x: i64, y: &mut i64) -> bool {
    let (result, overflowed) = x.overflowing_add(*y);
    *y = result;
    overflowed
}

pub fn cap_add_to(x: i64, y: &mut i64) {
    *y = cap_add(*y, x);
}

pub fn cap_sub(x: i64, y: i64) -> i64 {
    cap_sub_built_in(x, y)
}

/// Updates *target with cap_sub(*target, amount).
pub fn cap_sub_from(amount: i64, target: &mut i64) {
    *target = cap_sub(*target, amount);
}

pub fn cap_prod(x: i64, y: i64) -> i64 {
    cap_prod_built_in(x, y)
}

/// Computes *result = x * y, and returns true iff there is an overflow or if
/// the result is kint64min.
pub fn mul_into_overflow(x: i64, y: i64, result: &mut i64) -> bool {
    let (product, overflowed) = x.overflowing_mul(y);
    *result = product;
    overflowed || *result == i64::MIN
}

impl CapOrFloatAdd for i64 {
    fn cap_or_float_add(x: i64, y: i64) -> i64 {
        cap_add(x, y)
    }
}

impl CapOrFloatAdd for i32 {
    fn cap_or_float_add(x: i32, y: i32) -> i32 {
        let sum = i64::from(x) + i64::from(y);
        if sum > i64::from(i32::MAX) {
            i32::MAX
        } else if sum < i64::from(i32::MIN) {
            i32::MIN
        } else {
            sum as i32
        }
    }
}

impl CapOrFloatAdd for f64 {
    fn cap_or_float_add(x: f64, y: f64) -> f64 {
        x + y
    }
}

impl CapOrFloatAdd for f32 {
    fn cap_or_float_add(x: f32, y: f32) -> f32 {
        x + y
    }
}

pub fn cap_or_float_add<T: CapOrFloatAdd>(x: T, y: T) -> T {
    T::cap_or_float_add(x, y)
}

#[cfg(test)]
mod tests {
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::{RngCore, SeedableRng};

    use super::*;

    #[derive(Clone, Copy)]
    struct TestType(i64);

    impl StrongInt64 for TestType {
        fn value(self) -> i64 {
            self.0
        }
        fn from_value(value: i64) -> Self {
            TestType(value)
        }
    }

    fn test_safe_add64(a: i64, b: i64, error_expected: bool) {
        let mut result_b = TestType(b);
        let mut result_a = TestType(a);
        assert_eq!(
            safe_add_into(TestType(a), &mut result_b),
            !error_expected,
            "{a} + {b}"
        );
        assert_eq!(
            safe_add_into(TestType(b), &mut result_a),
            !error_expected,
            "{a} + {b}"
        );
        if !error_expected {
            assert_eq!(result_a.value(), a + b, "{a} + {b}");
            assert_eq!(result_b.value(), a + b, "{a} + {b}");
        }
    }

    #[test]
    fn safe_add_test_basic_cases() {
        test_safe_add64(5, 42, /* error_expected */ false);
        test_safe_add64(i64::MIN / 2, i64::MIN / 2, /* error_expected= */ false);
        test_safe_add64(i64::MAX / 2, i64::MAX / 2, /* error_expected= */ false);
        test_safe_add64(
            i64::MIN / 2,
            i64::MIN / 2 - 2,
            /* error_expected= */ true,
        );
        test_safe_add64(
            i64::MAX / 2,
            i64::MAX / 2 + 2,
            /* error_expected= */ true,
        );
        test_safe_add64(
            i64::MIN / 2,
            i64::MIN / 2 - 1,
            /* error_expected= */ true,
        );
        test_safe_add64(
            i64::MAX / 2,
            i64::MAX / 2 + 1,
            /* error_expected= */ false,
        );
        test_safe_add64(i64::MIN, -1, /* error_expected= */ true);
        test_safe_add64(i64::MAX, 1, /* error_expected= */ true);
        test_safe_add64(i64::MIN, 1, /* error_expected= */ false);
        test_safe_add64(i64::MAX, -1, /* error_expected= */ false);
    }

    fn generate_int64_values_for_testing() -> Vec<i64> {
        let mut v = Vec::new();
        // Generate a bunch of special values...
        for base in [
            0,
            i64::MIN,
            i64::MAX,
            i64::from(i32::MAX),
            i64::from(i32::MIN),
        ] {
            // ...scaled by various factor...
            for scaled in [base, base / 2, base / 3 * 2] {
                // ...with small offsets added.
                for offset in [-1000, -2, -1, 0, 1, 2, 1000] {
                    // Avoid generating an overflow here. Do not want saturated as we
                    // want corner-case numbers, not saturated arithmetic.
                    v.push(twos_complement_addition(scaled, offset));
                }
            }
        }
        v
    }

    // The fixture of the upstream tests: the values every case runs over.
    struct SaturatedArithmeticTest {
        int64_values: Vec<i64>,
    }

    // Upstream builds the reference results from hand made 128 bit helpers, since it has no
    // signed 128 bit type there. Rust has `i128`, so the reference clamps an `i128` result.
    fn int128_to_capped64(x: i128) -> i64 {
        x.clamp(i128::from(i64::MIN), i128::from(i64::MAX)) as i64
    }

    // Reference (safe, but slow) versions of the functions whose behavior we are testing.
    fn reference_cap_add(a: i64, b: i64) -> i64 {
        int128_to_capped64(i128::from(a) + i128::from(b))
    }
    fn reference_cap_sub(a: i64, b: i64) -> i64 {
        int128_to_capped64(i128::from(a) - i128::from(b))
    }
    fn reference_cap_opp(v: i64) -> i64 {
        int128_to_capped64(-i128::from(v))
    }
    fn reference_cap_prod(a: i64, b: i64) -> i64 {
        int128_to_capped64(i128::from(a) * i128::from(b))
    }

    impl SaturatedArithmeticTest {
        fn set_up() -> Self {
            let mut int64_values = generate_int64_values_for_testing();
            // Now, add a few random values.
            const K_NUM_RANDOM_VALUES: i32 = 1000;
            let mut random = ChaCha8Rng::seed_from_u64(12345);
            for _ in 0..K_NUM_RANDOM_VALUES {
                int64_values.push(random.next_u64() as i64);
            }
            Self { int64_values }
        }
    }

    // Don't use assert_eq to print every pair: we test ~1M pairs of values.
    macro_rules! test_binary_operator {
        ($test_name:ident, $name:ident, $symbol:expr, $reference:ident) => {
            #[test]
            fn $test_name() {
                let fixture = SaturatedArithmeticTest::set_up();
                for &a in &fixture.int64_values {
                    for &b in &fixture.int64_values {
                        assert_eq!($reference(a, b), $name(a, b), "{} {} {}", a, $symbol, b);
                    }
                }
            }
        };
    }

    test_binary_operator!(
        saturated_arithmetic_test_cap_add,
        cap_add,
        "+",
        reference_cap_add
    );
    test_binary_operator!(
        saturated_arithmetic_test_cap_add_generic,
        cap_add_generic,
        "+",
        reference_cap_add
    );
    test_binary_operator!(
        saturated_arithmetic_test_cap_sub,
        cap_sub,
        "-",
        reference_cap_sub
    );
    test_binary_operator!(
        saturated_arithmetic_test_cap_sub_generic,
        cap_sub_generic,
        "-",
        reference_cap_sub
    );
    test_binary_operator!(
        saturated_arithmetic_test_cap_prod,
        cap_prod,
        "*",
        reference_cap_prod
    );
    test_binary_operator!(
        saturated_arithmetic_test_cap_prod_generic,
        cap_prod_generic,
        "*",
        reference_cap_prod
    );

    test_binary_operator!(
        saturated_arithmetic_test_cap_add_built_in,
        cap_add_built_in,
        "+",
        reference_cap_add
    );
    test_binary_operator!(
        saturated_arithmetic_test_cap_sub_built_in,
        cap_sub_built_in,
        "-",
        reference_cap_sub
    );
    test_binary_operator!(
        saturated_arithmetic_test_cap_prod_built_in,
        cap_prod_built_in,
        "*",
        reference_cap_prod
    );

    #[test]
    fn saturated_arithmetic_test_cap_opp() {
        let fixture = SaturatedArithmeticTest::set_up();
        for &a in &fixture.int64_values {
            assert_eq!(reference_cap_opp(a), cap_opp(a), "{a}");
        }
    }

    #[test]
    fn saturated_arithmetic_test_cap_abs() {
        assert_eq!(cap_abs(i64::MIN), i64::MAX);
        assert_eq!(cap_abs(i64::MAX), i64::MAX);
        assert_eq!(cap_abs(-100), 100);
        assert_eq!(cap_abs(0), 0);
        assert_eq!(cap_abs(200), 200);
    }

    #[test]
    fn saturated_arithmetic_test_cap_add_to() {
        let fixture = SaturatedArithmeticTest::set_up();
        for &a in &fixture.int64_values {
            for &b in &fixture.int64_values {
                let mut result = a;
                cap_add_to(b, &mut result);
                assert_eq!(result, cap_add(a, b), "{a} + {b}");
            }
        }
    }

    #[test]
    fn saturated_arithmetic_test_add_into_overflow() {
        let fixture = SaturatedArithmeticTest::set_up();
        for &a in &fixture.int64_values {
            for &b in &fixture.int64_values {
                let mut result = b;
                if add_into_overflow(a, &mut result) {
                    assert!(add_overflows(a, b));
                } else {
                    assert!(!add_overflows(a, b));
                    assert_eq!(result, a + b);
                }
            }
        }
    }

    #[test]
    fn saturated_arithmetic_test_cap_sub_from() {
        let fixture = SaturatedArithmeticTest::set_up();
        for &a in &fixture.int64_values {
            for &b in &fixture.int64_values {
                let mut result = a;
                cap_sub_from(/* amount= */ b, /* target= */ &mut result);
                assert_eq!(result, cap_sub(a, b), "{a} + {b}");
            }
        }
    }

    #[test]
    fn saturated_arithmetic_test_mul_into_overflow() {
        let fixture = SaturatedArithmeticTest::set_up();
        let mut values = fixture.int64_values.clone();
        for extra in [
            -7,
            7,
            i64::MAX / 7,
            -(i64::MAX / 7),
            i64::MAX / 7 + 1,
            -(i64::MAX / 7 + 1),
            i64::MAX / 7 - 1,
            -(i64::MAX / 7 - 1),
            3_i64 << 60,
            -(3_i64 << 60),
            3_i64 << 61,
            -(3_i64 << 61),
        ] {
            values.push(extra);
        }
        for k in 0..63 {
            values.push(1_i64 << k);
            values.push(-(1_i64 << k));
        }

        for &a in &values {
            for &b in &values {
                let exact = i128::from(a) * i128::from(b);
                let expected_overflow =
                    exact <= i128::from(i64::MIN) || exact > i128::from(i64::MAX);

                let mut result = 0;
                let overflow = mul_into_overflow(a, b, &mut result);
                assert_eq!(overflow, expected_overflow, "{a} * {b}");
                if !overflow {
                    assert_ne!(result, i64::MIN, "{a} * {b}");
                    assert_eq!(result, exact as i64, "{a} * {b}");
                }

                // Also verify the cap_prod-based fallback used without builtins.
                let prod = cap_prod(a, b);
                let fallback_overflow = prod == i64::MIN || (prod == i64::MAX && i64::MAX / a != b);
                assert_eq!(fallback_overflow, expected_overflow, "{a} * {b}");
                if !fallback_overflow {
                    assert_eq!(prod, exact as i64, "{a} * {b}");
                }
            }
        }
    }

    #[test]
    fn cap_or_float_add_various_types() {
        const K_INF: f64 = f32::INFINITY as f64;
        assert_eq!(cap_or_float_add(0.5, 0.25), 0.75);
        assert_eq!(cap_or_float_add(K_INF, 0.25), K_INF);
        assert_eq!(cap_or_float_add(1_i64, i64::MAX), i64::MAX);
        assert_eq!(cap_or_float_add(4, i32::MAX - 2), i32::MAX);

        // A corner case: float isn't accurate enough to represent 1+1e-10, which we
        // use to verify that the return type is the same as the argument type.
        assert_eq!(f64::from(cap_or_float_add(1_f32, 1e-10_f32)), 1.0);
    }

    // TODO(user): do the same for cap_opp.

    #[test]
    fn cap_functions_equal_the_clamped_i128_result() {
        // Arrange
        let mut random = ChaCha8Rng::seed_from_u64(12345);
        let special = [i64::MIN, i64::MAX, 0, 1, -1];
        let mut pairs: Vec<(i64, i64)> = Vec::new();
        for &a in &special {
            for &b in &special {
                pairs.push((a, b));
            }
        }
        for _ in 0..100_000 {
            pairs.push((random.next_u64() as i64, random.next_u64() as i64));
        }

        for (a, b) in pairs {
            // Act and Assert
            assert_eq!(cap_add(a, b), reference_cap_add(a, b), "{a} + {b}");
            assert_eq!(cap_sub(a, b), reference_cap_sub(a, b), "{a} - {b}");
            assert_eq!(cap_prod(a, b), reference_cap_prod(a, b), "{a} * {b}");
        }
    }
}
