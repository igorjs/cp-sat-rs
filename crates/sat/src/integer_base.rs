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
// Ported from ortools/sat/integer_base.h at commit c931f5f7. Changed: translated from C++ to Rust.

use cpsat_util::define_strong_int64_type;
use cpsat_util::saturated_arithmetic::{self, cap_prod, cap_sub};

// Value type of an integer variable. An integer variable is always bounded
// on both sides, and this type is also used to store the bounds [lb, ub] of the
// range of each integer variable.
//
// Note that both bounds are inclusive, which allows to write many propagation
// algorithms for just one of the bound and apply it to the negated variables to
// get the symmetric algorithm for the other bound.
define_strong_int64_type!(pub IntegerValue);

// The max range of an integer variable is [kMinIntegerValue, kMaxIntegerValue].
//
// It is symmetric so the set of possible ranges stays the same when we take the
// negation of a variable. Moreover, we need some IntegerValue that fall outside
// this range on both side so that we can usually take care of integer overflow
// by simply doing "saturated arithmetic" and if one of the bound overflow, the
// two bounds will "cross" each others and we will get an empty range.
pub const K_MAX_INTEGER_VALUE: IntegerValue = IntegerValue::new(i64::MAX - 1);
pub const K_MIN_INTEGER_VALUE: IntegerValue = IntegerValue::new(-K_MAX_INTEGER_VALUE.value());

// Overflows and saturated arithmetic.

pub fn cap_prod_i(a: IntegerValue, b: IntegerValue) -> IntegerValue {
    IntegerValue::new(cap_prod(a.value(), b.value()))
}

pub fn cap_sub_i(a: IntegerValue, b: IntegerValue) -> IntegerValue {
    IntegerValue::new(cap_sub(a.value(), b.value()))
}

// Computes *result = a * b, and returns true iff there is an overflow or if
// the result is kint64min.
pub fn mul_into_overflow(a: IntegerValue, b: IntegerValue, result: &mut IntegerValue) -> bool {
    let mut product: i64 = 0;
    let overflow = saturated_arithmetic::mul_into_overflow(a.value(), b.value(), &mut product);
    *result = IntegerValue::new(product);
    overflow
}

// Returns true iff t * value overflows or is kint64min.
pub fn prod_overflow(t: IntegerValue, value: IntegerValue) -> bool {
    let mut prod = IntegerValue::new(0);
    mul_into_overflow(t, value, &mut prod)
}

// Computes *result += a, and returns false iff there is an overflow or if the
// result is kint64min.
pub fn add_to(a: IntegerValue, result: &mut IntegerValue) -> bool {
    let mut add: i64 = result.value();
    if saturated_arithmetic::add_into_overflow(a.value(), &mut add) || add == i64::MIN {
        return false;
    }
    *result = IntegerValue::new(add);
    true
}

// Computes *result += a * b, and returns false iff a * b or the final result
// overflows int64_t or equals kint64min.
pub fn add_product_to(a: IntegerValue, b: IntegerValue, result: &mut IntegerValue) -> bool {
    let mut prod = IntegerValue::new(0);
    if mul_into_overflow(a, b, &mut prod) {
        return false;
    }
    add_to(prod, result)
}

// Computes *result += a * a, and returns false iff a * a or the final result
// overflows int64_t or equals kint64min.
pub fn add_square_to(a: IntegerValue, result: &mut IntegerValue) -> bool {
    add_product_to(a, a, result)
}

#[cfg(test)]
mod tests {
    use super::*;

    // PENDING(phase 2): TEST(CanonicalizeAffinePrecedenceTest, Basic), needs sat/integer_base.h
    // PENDING(phase 2): TEST(CanonicalizeAffinePrecedenceTest, OneSingleVariable), needs sat/integer_base.h
    // PENDING(phase 2): TEST(BestBinaryRelationBoundsTest, Basic), needs sat/integer_base.h
    // PENDING(phase 2): TEST(Linear2BoundAffineRelaxationTest, Random), needs sat/integer_base.h
    // PENDING(phase 2): TEST(LinearExpression2Test, GetDifferenceLowerBound), needs sat/integer_base.h

    #[test]
    fn integer_base_overflow_test_mul_and_add_overflow_helpers() {
        const K_MIN: i64 = i64::MIN;
        const K_MAX: i64 = i64::MAX;
        let mut values: Vec<i64> = vec![
            K_MIN,
            K_MIN + 1,
            K_MIN + 2,
            -1000,
            -7,
            -3,
            -2,
            -1,
            0,
            1,
            2,
            3,
            7,
            1000,
            K_MAX - 2,
            K_MAX - 1,
            K_MAX,
            K_MAX / 7,
            -(K_MAX / 7),
            K_MAX / 7 + 1,
            -(K_MAX / 7 + 1),
            K_MAX / 7 - 1,
            -(K_MAX / 7 - 1),
            3_i64 << 60,
            -(3_i64 << 60),
            3_i64 << 61,
            -(3_i64 << 61),
        ];
        for k in 0..63 {
            values.push(1_i64 << k);
            values.push(-(1_i64 << k));
        }

        for &a in &values {
            for &b in &values {
                let exact_prod = a as i128 * b as i128;
                let expected_prod_overflow =
                    exact_prod <= K_MIN as i128 || exact_prod > K_MAX as i128;
                assert_eq!(
                    prod_overflow(IntegerValue::new(a), IntegerValue::new(b)),
                    expected_prod_overflow,
                    "{a} * {b}"
                );
                let mut prod_result = IntegerValue::new(0);
                assert_eq!(
                    mul_into_overflow(IntegerValue::new(a), IntegerValue::new(b), &mut prod_result),
                    expected_prod_overflow,
                    "{a} * {b}"
                );
                if !expected_prod_overflow {
                    assert_ne!(prod_result.value(), K_MIN);
                    assert_eq!(prod_result.value(), exact_prod as i64);
                    assert_ne!((-prod_result).value(), K_MIN);
                }

                let exact_sum = a as i128 + b as i128;
                let expected_add_ok = exact_sum > K_MIN as i128 && exact_sum <= K_MAX as i128;
                let mut sum_result = IntegerValue::new(b);
                assert_eq!(
                    add_to(IntegerValue::new(a), &mut sum_result),
                    expected_add_ok,
                    "{a} + {b}"
                );
                if expected_add_ok {
                    assert_ne!(sum_result.value(), K_MIN);
                    assert_eq!(sum_result.value(), exact_sum as i64);
                    // Negating any non-overflowing result stays in [-kMax, +kMax].
                    assert_ne!((-sum_result).value(), K_MIN);
                } else {
                    assert_eq!(sum_result, IntegerValue::new(b));
                }
            }
        }

        // Inner product overflow must still fail AddProductTo even if adding to
        // *result would bring the mathematical total back into [-kMax, kMax].
        let mut acc = IntegerValue::new(-100);
        assert!(!add_product_to(
            IntegerValue::new(1_i64 << 62),
            IntegerValue::new(2),
            &mut acc
        ));
        assert_eq!(acc, IntegerValue::new(-100));

        acc = IntegerValue::new(100);
        assert!(!add_product_to(
            IntegerValue::new(-(1_i64 << 62)),
            IntegerValue::new(2),
            &mut acc
        ));
        assert_eq!(acc, IntegerValue::new(100));

        // Exact +kint64max and -kint64max succeed.
        acc = IntegerValue::new(0);
        assert!(add_product_to(
            IntegerValue::new(7),
            IntegerValue::new(K_MAX / 7),
            &mut acc
        ));
        assert_eq!(acc, IntegerValue::new(K_MAX));

        acc = IntegerValue::new(0);
        assert!(add_product_to(
            IntegerValue::new(-7),
            IntegerValue::new(K_MAX / 7),
            &mut acc
        ));
        assert_eq!(acc, IntegerValue::new(-K_MAX));

        acc = IntegerValue::new(0);
        assert!(add_square_to(IntegerValue::new(1_i64 << 31), &mut acc));
        assert_eq!(acc, IntegerValue::new(1_i64 << 62));
        assert!(!add_square_to(IntegerValue::new(1_i64 << 32), &mut acc));
    }

    #[test]
    fn integer_value_arithmetic_and_ordering_equal_those_of_the_inner_integers() {
        // Arrange
        let pairs: [(i64, i64); 5] = [(3, 4), (-7, 5), (0, -2), (1000, 1000), (-9, -9)];
        for (a, b) in pairs {
            let (x, y) = (IntegerValue::new(a), IntegerValue::new(b));

            // Act and Assert
            assert_eq!((x + y).value(), a + b);
            assert_eq!((x - y).value(), a - b);
            assert_eq!((x * y).value(), a * b);
            assert_eq!(x < y, a < b);
            assert_eq!(x <= y, a <= b);
            assert_eq!(x == y, a == b);
            assert_eq!(x.cmp(&y), a.cmp(&b));
            assert_eq!(x.max(y).value(), a.max(b));
            assert_eq!(x.min(y).value(), a.min(b));
        }
    }

    #[test]
    fn integer_value_limits_are_symmetric_and_inside_the_integer_range() {
        // Arrange
        let max = K_MAX_INTEGER_VALUE;
        let min = K_MIN_INTEGER_VALUE;

        // Act and Assert
        assert_eq!(max.value(), i64::MAX - 1);
        assert_eq!(min.value(), -(i64::MAX - 1));
        assert_eq!(-max, min);
        assert!(min.value() > i64::MIN);
    }

    #[test]
    fn cap_prod_i_and_cap_sub_i_saturate_like_the_inner_functions() {
        // Arrange
        let big = IntegerValue::new(i64::MAX / 2 + 1);
        let small = IntegerValue::new(-5);

        // Act and Assert
        assert_eq!(cap_prod_i(IntegerValue::new(6), small).value(), -30);
        assert_eq!(cap_prod_i(big, IntegerValue::new(2)).value(), i64::MAX);
        assert_eq!(cap_prod_i(big, IntegerValue::new(-2)).value(), i64::MIN);
        assert_eq!(cap_sub_i(IntegerValue::new(3), small).value(), 8);
        assert_eq!(
            cap_sub_i(IntegerValue::new(i64::MIN + 1), big).value(),
            i64::MIN
        );
        assert_eq!(
            cap_sub_i(big, IntegerValue::new(i64::MIN + 1)).value(),
            i64::MAX
        );
    }
}
