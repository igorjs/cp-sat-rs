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
// Ported from ortools/sat/util.h at commit c931f5f7. Changed: translated from C++ to Rust.
// Ported from ortools/sat/util.cc at commit c931f5f7. Changed: translated from C++ to Rust.

use cpsat_util::sorted_interval_list::Domain;

use crate::cp_model_utils::std_gcd;

// This will be optimized into one division. I tested that in
// `BM_DivisionAndRemainderAlternative` (sat/integer_test.cc)
//
// Note that I am not 100% sure we need the indirection for the optimization
// to kick in though, but this seemed safer given our weird r[i ^ 1] inputs.
fn quotient_and_remainder(a: i64, b: i64) -> (i64, i64) {
    (a / b, a % b)
}

// Returns a in [0, m) such that a * x = 1 modulo m.
// If gcd(x, m) != 1, there is no inverse, and it returns 0.
//
// This DCHECKs that x is in [0, m).
// This is integer overflow safe.
//
// NOTE(user): I didn't find this in an easily usable standard library.
//
// Using the extended Euclidean algo, we find a and b such that
//     a x + b m = gcd(x, m)
// https://en.wikipedia.org/wiki/Extended_Euclidean_algorithm
pub fn modular_inverse(x: i64, m: i64) -> i64 {
    debug_assert!(x >= 0);
    debug_assert!(x < m);

    let mut r: [i64; 2] = [m, x];
    let mut t: [i64; 2] = [0, 1];

    // We only keep the last two terms of the sequences with the "^1" trick:
    //
    // q = r[i-2] / r[i-1]
    // r[i] = r[i-2] % r[i-1]
    // t[i] = t[i-2] - t[i-1] * q
    //
    // We always have:
    // - gcd(r[i], r[i - 1]) = gcd(r[i - 1], r[i - 2])
    // - x * t[i] + m * t[i - 1] = r[i]
    let mut i = 0;
    while r[i ^ 1] != 0 {
        let (q, remainder) = quotient_and_remainder(r[i], r[i ^ 1]);
        r[i] = remainder;
        t[i] -= t[i ^ 1] * q;
        i ^= 1;
    }

    // If the gcd is not one, there is no inverse, we return 0.
    if r[i] != 1 {
        return 0;
    }

    // Correct the result so that it is in [0, m). Note that abs(t[i]) is known to
    // be less than or equal to x / 2, and we have thorough unit-tests.
    if t[i] < 0 {
        t[i] += m;
    }

    t[i]
}

// Just returns x % m but with a result always in [0, m).
pub fn positive_mod(x: i64, m: i64) -> i64 {
    let r = x % m;
    if r < 0 { r + m } else { r }
}

// If we know that X * coeff % mod = rhs % mod, this returns c such that
// PositiveMod(X, mod) = c.
//
// This requires coeff != 0, mod !=0 and gcd(coeff, mod) == 1.
// The result will be in [0, mod) but there is no other condition on the sign or
// magnitude of a and b.
//
// This is overflow safe, and when rhs == 0 or abs(mod) == 1, it returns 0.
pub fn product_with_modular_inverse(coeff: i64, mod_: i64, rhs: i64) -> i64 {
    debug_assert_ne!(coeff, 0);
    debug_assert_ne!(mod_, 0);

    let mod_ = mod_.abs();
    if rhs == 0 || mod_ == 1 {
        return 0;
    }
    debug_assert_eq!(std_gcd(coeff.abs(), mod_), 1);

    // Make both in [0, mod).
    let coeff = positive_mod(coeff, mod_);
    let rhs = positive_mod(rhs, mod_);

    // From X * coeff % mod = rhs
    // We deduce that X % mod = rhs * inverse % mod
    let inverse = modular_inverse(coeff, mod_);
    assert_ne!(inverse, 0);

    // We make the operation in 128 bits to be sure not to have any overflow here.
    let p = i128::from(inverse) * i128::from(rhs);
    (p % i128::from(mod_)) as i64
}

// Returns true if the equation a * X + b * Y = cte has some integer solutions.
// For now, we check that a and b are different from 0 and from int64_t min.
//
// There is actually always a solution if cte % gcd(|a|, |b|) == 0. And because
// a, b and cte fit on an int64_t, if there is a solution, there is one with X
// and Y fitting on an int64_t.
//
// We will divide everything by gcd(a, b) first, so it is why we take reference
// and the equation can change.
//
// If there are solutions, we return one of them (x0, y0).
// From any such solution, the set of all solutions is given for Z integer by:
// X = x0 + b * Z;
// Y = y0 - a * Z;
//
// Given a domain for X and Y, it is possible to compute the "exact" domain of Z
// with our Domain functions. Note however that this will only compute solutions
// where both x-x0 and y-y0 do fit on an int64_t:
// DomainOf(x).SubtractionWith(x0).InverseMultiplicationBy(b).IntersectionWith(
//     DomainOf(y).SubtractionWith(y0).InverseMultiplicationBy(-a))
pub fn solve_diophantine_equation_of_size_two(
    a: &mut i64,
    b: &mut i64,
    cte: &mut i64,
    x0: &mut i64,
    y0: &mut i64,
) -> bool {
    assert_ne!(*a, 0);
    assert_ne!(*b, 0);
    assert_ne!(*a, i64::MIN);
    assert_ne!(*b, i64::MIN);

    let gcd = std_gcd(a.abs(), b.abs());
    if *cte % gcd != 0 {
        return false;
    }
    *a /= gcd;
    *b /= gcd;
    *cte /= gcd;

    // The simple case where (0, 0) is a solution.
    if *cte == 0 {
        *x0 = 0;
        *y0 = 0;
        return true;
    }

    // We solve a * X + b * Y = cte
    // We take a valid x0 in [0, b) by considering the equation mod b.
    *x0 = product_with_modular_inverse(*a, *b, *cte);

    // We choose x0 of the same sign as cte.
    if *cte < 0 && *x0 != 0 {
        *x0 -= b.abs();
    }

    // By plugging X = x0 + b * Z
    // We have a * (x0 + b * Z) + b * Y = cte
    // so a * b * Z + b * Y = cte - a * x0;
    // and y0 = (cte - a * x0) / b (with an exact division by construction).
    let t = i128::from(*cte) - i128::from(*a) * i128::from(*x0);
    debug_assert_eq!(t % i128::from(*b), 0);

    // Overflow-wise, there are two cases for cte > 0:
    // - a * x0 <= cte, in this case y0 will not overflow (<= cte).
    // - a * x0 > cte, in this case y0 will be in (-a, 0].
    let r = t / i128::from(*b);
    debug_assert!(r <= i128::from(i64::MAX));
    debug_assert!(r >= i128::from(i64::MIN));

    *y0 = r as i64;
    true
}

// Returns true if the equation a * X + b * Y = cte has some integer solutions
// in the domain of X and Y.
//
// Signed overflow of `b * div` and `a * div` is undefined upstream; here it panics.
pub fn diophantine_equation_of_size_two_has_solution_in_domain(
    x: &Domain,
    a: i64,
    y: &Domain,
    b: i64,
    cte: i64,
) -> bool {
    if x.is_empty() || y.is_empty() {
        return false;
    }
    if a == 0 && b == 0 {
        return cte == 0;
    }
    if a == 0 {
        let div = cte / b;
        if b * div != cte {
            return false;
        }
        return y.contains(div);
    }
    if b == 0 {
        let div = cte / a;
        if a * div != cte {
            return false;
        }
        return x.contains(div);
    }

    if a == b {
        let div = cte / a;
        if a * div != cte {
            return false;
        }
        return !x
            .negation()
            .addition_with(&Domain::new_value(div))
            .intersection_with(y)
            .is_empty();
    }

    let (mut a, mut b, mut cte) = (a, b, cte);
    let (mut x0, mut y0) = (0, 0);
    if !solve_diophantine_equation_of_size_two(&mut a, &mut b, &mut cte, &mut x0, &mut y0) {
        return false;
    }
    let _z_domain = x
        .addition_with(&Domain::new_value(-x0))
        .inverse_multiplication_by(b)
        .intersection_with(
            &y.addition_with(&Domain::new_value(-y0))
                .inverse_multiplication_by(-a),
        );

    let z_restricted_d1 = x
        .addition_with(&Domain::new_value(-x0))
        .inverse_multiplication_by(b);
    let z_restricted_d2 = y
        .addition_with(&Domain::new_value(-y0))
        .inverse_multiplication_by(-a);
    let z_restricted_domain = z_restricted_d1.intersection_with(&z_restricted_d2);
    !z_restricted_domain.is_empty()
}

#[cfg(test)]
mod tests {
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::{RngCore, SeedableRng};

    use super::*;

    // PENDING(phase 1): TEST(CompactVectorVectorTest, EmptyCornerCases), needs sat/util.h
    // PENDING(phase 1): TEST(CompactVectorVectorTest, ResetFromFlatMapping), needs sat/util.h
    // PENDING(phase 1): TEST(CompactVectorVectorTest, ResetFromFlatMappingWithMinimumNumNodes), needs sat/util.h
    // PENDING(phase 1): TEST(CompactVectorVectorTest, MoveOnlyType), needs sat/util.h
    // PENDING(phase 1): TEST(CompactVectorVectorTest, MoveOnlyTypeWithBuilder), needs sat/util.h
    // PENDING(phase 1): TEST(CompactVectorVectorTest, RemoveBySwap), needs sat/util.h
    // PENDING(phase 1): TEST(CompactVectorVectorTest, ShrinkValues), needs sat/util.h
    // PENDING(phase 1): TEST(CompactVectorVectorTest, SortAndRemoveDuplicateValues), needs sat/util.h
    // PENDING(phase 1): TEST(CompactVectorVectorTest, ResetFromTranspose), needs sat/util.h
    // PENDING(phase 1): TEST(MergeableOccurrenceList, BasicTest), needs sat/util.h
    // PENDING(phase 1): TEST(FormatTable, BasicAlign), needs sat/util.h

    // A draw in [lo, hi), as `absl::Uniform(random, lo, hi)`.
    fn uniform(random: &mut ChaCha8Rng, lo: i64, hi: i64) -> i64 {
        lo + (random.next_u64() % (hi - lo) as u64) as i64
    }

    // A draw in [lo, hi] that is uniform on the number of bits first, as `absl::LogUniform`.
    fn log_uniform(random: &mut ChaCha8Rng, lo: i64, hi: i64) -> i64 {
        let range = (hi - lo) as u64;
        let bits = 64 - range.leading_zeros();
        let limit = match uniform(random, 0, i64::from(bits) + 1) {
            0 => 0,
            e => ((1u64 << e) - 1).min(range),
        };
        lo + (random.next_u64() % (limit + 1)) as i64
    }

    // True with probability `p`, as `absl::Bernoulli(random, p)`.
    fn bernoulli(random: &mut ChaCha8Rng, p: f64) -> bool {
        ((random.next_u64() >> 11) as f64) / ((1u64 << 53) as f64) < p
    }

    #[test]
    fn modular_inverse_test_all_small_values() {
        // Arrange
        // Act
        for m in 1..1000_i64 {
            for x in 1..m {
                let inverse = modular_inverse(x, m);

                // Assert
                assert!(inverse >= 0);
                assert!(inverse < m);
                if inverse == 0 {
                    assert_ne!(std_gcd(x, m), 1);
                } else {
                    assert_eq!(x * inverse % m, 1);
                }
            }
        }
    }

    #[test]
    fn modular_inverse_test_basic_overflow_test() {
        // Arrange
        let mut random = ChaCha8Rng::seed_from_u64(12345);
        let max = i64::MAX;

        for _ in 0..100000 {
            let m = max - log_uniform(&mut random, 0, max);
            let x = uniform(&mut random, 0, m);

            // Act
            let inverse = modular_inverse(x, m);

            // Assert
            assert!(inverse >= 0);
            assert!(inverse < m);
            if inverse == 0 {
                assert_ne!(std_gcd(x, m), 1);
            } else {
                let test_x = i128::from(x);
                let test_inverse = i128::from(inverse);
                let test_m = i128::from(m);
                assert_eq!(test_x * test_inverse % test_m, 1);
            }
        }
    }

    #[test]
    fn product_with_modular_inverse_test_few_small_values() {
        // Arrange
        let limit: i64 = 50;
        for mod_ in 1..limit {
            for coeff in -limit..limit {
                if coeff == 0 || std_gcd(mod_, coeff.abs()) != 1 {
                    continue;
                }
                for rhs in -mod_..mod_ {
                    // Act
                    let result = product_with_modular_inverse(coeff, mod_, rhs);

                    // Assert
                    for test in -limit..limit {
                        let x = test * mod_ + result;
                        assert_eq!(positive_mod(x * coeff, mod_), positive_mod(rhs, mod_));
                    }
                }
            }
        }
    }

    #[test]
    fn diophantine_equation_of_size_two_has_solution_in_domain_test_random_small_values() {
        // Arrange
        const K_LIMIT: i64 = 1000;
        const K_NUM_TRIES: i32 = 10000;
        let mut random = ChaCha8Rng::seed_from_u64(12345);
        for _ in 0..K_NUM_TRIES {
            let a = uniform(&mut random, -K_LIMIT, K_LIMIT);
            let b = uniform(&mut random, -K_LIMIT, K_LIMIT);
            let x = uniform(&mut random, -K_LIMIT, K_LIMIT);
            let y = uniform(&mut random, -K_LIMIT, K_LIMIT);

            let cte = a * x + b * y;

            // Act and Assert
            assert!(diophantine_equation_of_size_two_has_solution_in_domain(
                &Domain::new(x - 10, x + 10),
                a,
                &Domain::new(y - 10, y + 10),
                b,
                cte
            ));

            let mut has_solution = false;
            let new_x = uniform(&mut random, -K_LIMIT, K_LIMIT);
            let new_y = uniform(&mut random, -K_LIMIT, K_LIMIT);
            for d in -5..5_i64 {
                has_solution |= a * (new_x + d) + b * new_y == cte;
            }
            assert_eq!(
                diophantine_equation_of_size_two_has_solution_in_domain(
                    &Domain::new(new_x - 5, new_x + 4),
                    a,
                    &Domain::new_value(new_y),
                    b,
                    cte
                ),
                has_solution,
                "a: {a} b: {b} c: {cte} x: {new_x} y: {new_y}"
            );
        }
    }

    // PENDING(phase 1): TEST(ClosestMultipleTest, BasicCases), needs sat/util.h
    // PENDING(phase 1): TEST(LinearInequalityCanBeReducedWithClosestMultipleTest, BasicCase), needs sat/util.h
    // PENDING(phase 1): TEST(LinearInequalityCanBeReducedWithClosestMultipleTest, Random), needs sat/util.h
    // PENDING(phase 1): TEST(MoveOneUnprocessedLiteralLastTest, CorrectBehavior), needs sat/util.h
    // PENDING(phase 1): TEST(MoveOneUnprocessedLiteralLastTest, CorrectComplexity), needs sat/util.h
    // PENDING(phase 1): TEST(IncrementalAverage, PositiveData), needs sat/util.h
    // PENDING(phase 1): TEST(IncrementalAverage, NegativeData), needs sat/util.h
    // PENDING(phase 1): TEST(IncrementalAverage, MixedData), needs sat/util.h
    // PENDING(phase 1): TEST(IncrementalAverage, InitialFeed), needs sat/util.h
    // PENDING(phase 1): TEST(IncrementalAverage, Reset), needs sat/util.h
    // PENDING(phase 1): TEST(ExponentialMovingAverage, Average), needs sat/util.h
    // PENDING(phase 1): TEST(Percentile, BasicTest), needs sat/util.h
    // PENDING(phase 1): TEST(Percentile, RecordLimit), needs sat/util.h
    // PENDING(phase 1): TEST(Percentile, BasicTest2), needs sat/util.h
    // PENDING(phase 1): TEST(Percentile, RandomNumbers), needs sat/util.h
    // PENDING(phase 1): TEST(SafeDoubleToInt64Test, BasicCases), needs sat/util.h
    // PENDING(phase 1): TEST(MaxBoundedSubsetSumTest, LowMaxValue), needs sat/util.h
    // PENDING(phase 1): TEST(MaxBoundedSubsetSumTest, LowNumberOfElements), needs sat/util.h
    // PENDING(phase 1): TEST(MaxBoundedSubsetSumTest, FailBackToGcd), needs sat/util.h
    // PENDING(phase 1): TEST(MaxBoundedSubsetSumTest, SimpleMultiChoice), needs sat/util.h
    // PENDING(phase 1): TEST(FirstFewValuesTest, Basic), needs sat/util.h
    // PENDING(phase 1): TEST(FirstFewValuesTest, Overflow), needs sat/util.h
    // PENDING(phase 1): TEST(BasicKnapsackSolverTest, BasicFeasibleExample), needs sat/util.h
    // PENDING(phase 1): TEST(BasicKnapsackSolverTest, BasicInfeasibleExample), needs sat/util.h
    // PENDING(phase 1): TEST(BasicKnapsackSolverTest, RandomComparisonWithSolver), needs sat/util.h
    // PENDING(phase 1): TEST_P(CeilFloorTest, FloorOfRatioInt), needs sat/util.h
    // PENDING(phase 1): TEST_P(CeilFloorTest, FloorOfRatioInt128), needs sat/util.h
    // PENDING(phase 1): TEST_P(CeilFloorTest, CeilOfRatioInt), needs sat/util.h
    // PENDING(phase 1): TEST_P(CeilFloorTest, CeilOfRatioInt128), needs sat/util.h
    // PENDING(phase 1): TEST(TopNTest, BasicBehavior), needs sat/util.h
    // PENDING(phase 1): TEST(TopNTest, Random), needs sat/util.h
    // PENDING(phase 1): TEST(AtMostOneDecompositionTest, DetectFullClique), needs sat/util.h
    // PENDING(phase 1): TEST(AtMostOneDecompositionTest, DetectDisjointCliques), needs sat/util.h
    // PENDING(phase 1): TEST(WeightedPickTest, SizeOne), needs sat/util.h
    // PENDING(phase 1): TEST(WeightedPickTest, SimpleTest), needs sat/util.h
    // PENDING(phase 1): TEST(SortedSubsetSums, RandomTest), needs sat/util.h
    // PENDING(phase 1): TEST(SortedSubsetSums, CornerCase), needs sat/util.h
    // PENDING(phase 1): TEST(MaxBoundedSubsetSumExactTest, RandomTest), needs sat/util.h
    // PENDING(phase 1): TEST(MaxBoundedSubsetSumExactTest, UsedToFail), needs sat/util.h
    // PENDING(phase 1): TEST(MaxBoundedSubsetSumExactTest, CornerCase), needs sat/util.h
    // PENDING(phase 1): TEST(DagTopologicalSortIteratorTest, GenerateValidPermutations), needs sat/util.h
    // PENDING(phase 1): TEST(DagTopologicalSortIteratorTest, GenerateAllPermutations), needs sat/util.h
    // PENDING(phase 1): TEST(DagTopologicalSortIteratorTest, OnePrecedence), needs sat/util.h
    // PENDING(phase 1): TEST(DagTopologicalSortIteratorTest, ReversePrecedence), needs sat/util.h
    // PENDING(phase 1): TEST(DagTopologicalSortIteratorTest, RandomTest), needs sat/util.h
    // PENDING(phase 1): TEST(FindMostDiverseSubsetTest, Random), needs sat/util.h
    // PENDING(phase 1): TEST(FindMostDiverseSubsetTest, RandomButAlwaysPickZero), needs sat/util.h
    // PENDING(phase 1): TEST(HeuristicallySplitLongLinearTest, BasicExamples), needs sat/util.h
    // PENDING(phase 1): TEST(IsStrictlyIncludedTest, BasicExamples), needs sat/util.h

    #[test]
    fn solve_diophantine_equation_of_size_two_test_few_small_values() {
        // Arrange
        let limit: i64 = 50;
        for a in -limit..limit {
            if a == 0 {
                continue;
            }
            for b in -limit..limit {
                if b == 0 {
                    continue;
                }
                for c in -limit..limit {
                    let (mut ca, mut cb, mut cc) = (a, b, c);
                    let (mut x0, mut y0) = (0, 0);

                    // Act
                    let r = solve_diophantine_equation_of_size_two(
                        &mut ca, &mut cb, &mut cc, &mut x0, &mut y0,
                    );

                    // Assert
                    if !r {
                        // This is the only case.
                        let gcd = std_gcd(a.abs(), b.abs());
                        assert!(gcd > 1);
                        assert_ne!(c % gcd, 0);
                        continue;
                    }
                    assert_eq!(ca * x0 + cb * y0, cc);
                }
            }
        }
    }

    #[test]
    fn solve_diophantine_equation_of_size_two_test_basic_overflow_test() {
        // Arrange
        let mut random = ChaCha8Rng::seed_from_u64(12345);
        let max = i64::MAX;
        for _ in 0..100000 {
            let mut a = max - log_uniform(&mut random, 0, max);
            let mut b = max - log_uniform(&mut random, 0, max);
            let mut cte = uniform(&mut random, 0, max);
            if bernoulli(&mut random, 0.5) {
                a = -a;
            }
            if bernoulli(&mut random, 0.5) {
                b = -b;
            }
            if bernoulli(&mut random, 0.5) {
                cte = -cte;
            }
            let (mut x0, mut y0) = (0, 0);

            // Act
            let solved =
                solve_diophantine_equation_of_size_two(&mut a, &mut b, &mut cte, &mut x0, &mut y0);

            // Assert
            if !solved {
                // This is the only case.
                let gcd = std_gcd(a.abs(), b.abs());
                assert!(gcd > 1);
                assert_ne!(cte % gcd, 0);
                continue;
            }
            assert_eq!(
                i128::from(a) * i128::from(x0) + i128::from(b) * i128::from(y0),
                i128::from(cte)
            );
        }
    }
}
