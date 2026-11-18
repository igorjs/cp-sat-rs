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
// Ported from ortools/util/strong_integers.h at commit c931f5f7. Changed: translated from C++ to Rust.

//! Strongly typed integer types.
//!
//! `define_strong_index_type!` makes a newtype over `i32` with the operators of `StrongIndex`.
//! `define_strong_int64_type!` makes a newtype over `i64` with the operators of `StrongInt64`.
//! Two types made by the macros cannot be mixed, and nothing converts to or from the inner
//! integer by itself: use `new` and `value`.
//!
//! A value of one type is not accepted where another is expected:
//!
//! ```compile_fail,E0308
//! cpsat_util::define_strong_int64_type!(Cost);
//! cpsat_util::define_strong_int64_type!(Weight);
//!
//! fn takes_cost(_: Cost) {}
//! takes_cost(Weight::new(1));
//! ```
//!
//! The types may be public and carry attributes:
//!
//! ```
//! use cpsat_util::{define_strong_index_type, define_strong_int64_type};
//!
//! fn main() {
//!     define_strong_index_type!(
//!         /// A position in a list.
//!         pub Position
//!     );
//!     define_strong_int64_type!(
//!         /// An amount of money.
//!         pub Cents
//!     );
//!
//!     assert_eq!(Position::new(2) + 3, Position::new(5));
//!     assert_eq!((Cents::new(10) * 4).value(), 40);
//! }
//! ```

/// Makes `$name`, one operator on two values of the type and on the type with its integer on
/// either side, as `STRONG_TYPE_ARITHMETIC_OP` does.
#[doc(hidden)]
#[macro_export]
macro_rules! strong_type_arithmetic_op {
    ($name:ident, $int:ty, $trait:ident, $method:ident, $op:tt) => {
        impl ::core::ops::$trait<$name> for $name {
            type Output = $name;
            fn $method(self, id_2: $name) -> $name {
                $name(self.0 $op id_2.0)
            }
        }
        impl ::core::ops::$trait<$int> for $name {
            type Output = $name;
            fn $method(self, arg_val: $int) -> $name {
                $name(self.0 $op arg_val)
            }
        }
        impl ::core::ops::$trait<$name> for $int {
            type Output = $name;
            fn $method(self, id: $name) -> $name {
                $name(self $op id.0)
            }
        }
    };
}

/// Makes one compound assignment operator with the type and with its integer on the right, as
/// `STRONG_ASSIGNMENT_OP` does.
#[doc(hidden)]
#[macro_export]
macro_rules! strong_assignment_op {
    ($name:ident, $int:ty, $trait:ident, $method:ident, $op:tt) => {
        impl ::core::ops::$trait<$name> for $name {
            fn $method(&mut self, arg_value: $name) {
                self.0 $op arg_value.0;
            }
        }
        impl ::core::ops::$trait<$int> for $name {
            fn $method(&mut self, arg_value: $int) {
                self.0 $op arg_value;
            }
        }
    };
}

/// Makes the comparisons of the type with itself and with its integer on either side, as
/// `STRONG_TYPE_COMPARISON_OP` does.
#[doc(hidden)]
#[macro_export]
macro_rules! strong_type_comparison_ops {
    ($name:ident, $int:ty) => {
        impl ::core::cmp::PartialEq<$int> for $name {
            fn eq(&self, val: &$int) -> bool {
                self.0 == *val
            }
        }
        impl ::core::cmp::PartialEq<$name> for $int {
            fn eq(&self, id: &$name) -> bool {
                *self == id.0
            }
        }
        impl ::core::cmp::PartialOrd<$int> for $name {
            fn partial_cmp(&self, val: &$int) -> ::core::option::Option<::core::cmp::Ordering> {
                self.0.partial_cmp(val)
            }
        }
        impl ::core::cmp::PartialOrd<$name> for $int {
            fn partial_cmp(&self, id: &$name) -> ::core::option::Option<::core::cmp::Ordering> {
                self.partial_cmp(&id.0)
            }
        }
    };
}

/// Makes what `StrongIndex` and `StrongInt64` share: the type, `value()`, the increment and
/// decrement operators, unary plus and minus, the stream and hash support, and the limits.
#[doc(hidden)]
#[macro_export]
macro_rules! strong_type_common {
    ($(#[$meta:meta])* $vis:vis $name:ident, $int:ty) => {
        $(#[$meta])*
        #[derive(Clone, Copy, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
        $vis struct $name($int);

        #[allow(dead_code)]
        impl $name {
            /// The smallest value, `numeric_limits::min()` and `lowest()` upstream.
            pub const MIN: $name = $name(<$int>::MIN);
            /// The largest value, `numeric_limits::max()` upstream.
            pub const MAX: $name = $name(<$int>::MAX);

            pub const fn type_name() -> &'static str {
                stringify!($name)
            }

            pub const fn new(value: $int) -> $name {
                $name(value)
            }

            pub const fn value(self) -> $int {
                self.0
            }

            pub const fn unary_plus(self) -> $name {
                $name(self.0)
            }

            pub fn pre_increment(&mut self) -> &mut $name {
                self.0 += 1;
                self
            }

            pub fn post_increment(&mut self) -> $name {
                let temp = *self;
                self.0 += 1;
                temp
            }

            pub fn pre_decrement(&mut self) -> &mut $name {
                self.0 -= 1;
                self
            }

            pub fn post_decrement(&mut self) -> $name {
                let temp = *self;
                self.0 -= 1;
                temp
            }
        }

        impl ::core::ops::Neg for $name {
            type Output = $name;
            fn neg(self) -> $name {
                $name(-self.0)
            }
        }

        // The explicit conversion `operator ValueType()` upstream.
        impl ::core::convert::From<$name> for $int {
            fn from(id: $name) -> $int {
                id.0
            }
        }

        impl ::core::fmt::Debug for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Debug::fmt(&self.0, f)
            }
        }

        impl ::core::fmt::Display for $name {
            fn fmt(&self, f: &mut ::core::fmt::Formatter<'_>) -> ::core::fmt::Result {
                ::core::fmt::Display::fmt(&self.0, f)
            }
        }

        $crate::strong_type_comparison_ops!($name, $int);
    };
}

/// Defines a strong index type over `i32`, as `DEFINE_STRONG_INDEX_TYPE` does.
///
/// It supports `+=` and `-=`, `+`, `-`, `*` and `%` with its own type and with an `i32` on
/// either side, the unary `-`, the comparisons, and the increment and decrement methods.
/// Attributes, such as a doc comment, may come before the visibility.
#[macro_export]
macro_rules! define_strong_index_type {
    ($(#[$meta:meta])* $vis:vis $name:ident) => {
        $crate::strong_type_common!($(#[$meta])* $vis $name, i32);

        $crate::strong_assignment_op!($name, i32, AddAssign, add_assign, +=);
        $crate::strong_assignment_op!($name, i32, SubAssign, sub_assign, -=);

        $crate::strong_type_arithmetic_op!($name, i32, Add, add, +);
        $crate::strong_type_arithmetic_op!($name, i32, Sub, sub, -);
        $crate::strong_type_arithmetic_op!($name, i32, Mul, mul, *);
        $crate::strong_type_arithmetic_op!($name, i32, Rem, rem, %);
    };
}

/// Defines a strong integer type over `i64`, as `DEFINE_STRONG_INT64_TYPE` does.
///
/// It supports every integer operator with its own type and with an `i64` on either side,
/// the unary `-` and `!`, the comparisons, `mutable_value`, and the increment and decrement
/// methods. The type implements `StrongInt64` of `saturated_arithmetic`. `<<` keeps the bits
/// that fit, as C++20 does for signed types, and panics only for a shift amount at or above
/// the bit width. Attributes, such as a doc comment, may come before the visibility.
#[macro_export]
macro_rules! define_strong_int64_type {
    ($(#[$meta:meta])* $vis:vis $name:ident) => {
        $crate::strong_type_common!($(#[$meta])* $vis $name, i64);

        #[allow(dead_code)]
        impl $name {
            pub fn mutable_value(&mut self) -> &mut i64 {
                &mut self.0
            }
        }

        impl ::core::ops::Not for $name {
            type Output = $name;
            fn not(self) -> $name {
                $name(!self.0)
            }
        }

        impl $crate::saturated_arithmetic::StrongInt64 for $name {
            fn value(self) -> i64 {
                self.0
            }
            fn from_value(value: i64) -> $name {
                $name(value)
            }
        }

        $crate::strong_assignment_op!($name, i64, AddAssign, add_assign, +=);
        $crate::strong_assignment_op!($name, i64, SubAssign, sub_assign, -=);
        $crate::strong_assignment_op!($name, i64, MulAssign, mul_assign, *=);
        $crate::strong_assignment_op!($name, i64, DivAssign, div_assign, /=);
        $crate::strong_assignment_op!($name, i64, ShlAssign, shl_assign, <<=);
        $crate::strong_assignment_op!($name, i64, ShrAssign, shr_assign, >>=);
        $crate::strong_assignment_op!($name, i64, RemAssign, rem_assign, %=);

        $crate::strong_type_arithmetic_op!($name, i64, Add, add, +);
        $crate::strong_type_arithmetic_op!($name, i64, Sub, sub, -);
        $crate::strong_type_arithmetic_op!($name, i64, Mul, mul, *);
        $crate::strong_type_arithmetic_op!($name, i64, Div, div, /);
        $crate::strong_type_arithmetic_op!($name, i64, Shl, shl, <<);
        $crate::strong_type_arithmetic_op!($name, i64, Shr, shr, >>);
        $crate::strong_type_arithmetic_op!($name, i64, Rem, rem, %);
    };
}

#[cfg(test)]
mod tests {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    use cpsat_testutil::expect_death;

    use crate::saturated_arithmetic::safe_add_into;

    define_strong_index_type!(TestIndex);
    define_strong_index_type!(OtherIndex);
    define_strong_int64_type!(TestInt);
    define_strong_int64_type!(OtherInt);

    fn hash_of<T: Hash>(value: &T) -> u64 {
        let mut hasher = DefaultHasher::new();
        value.hash(&mut hasher);
        hasher.finish()
    }

    #[test]
    fn adding_two_values_of_an_index_type_gives_the_sum() {
        // Arrange
        let a = TestIndex::new(3);
        let b = TestIndex::new(4);
        // Act
        let sum = a + b;
        // Assert
        assert_eq!(sum, TestIndex::new(7));
        assert_eq!(sum.value(), 7);
    }

    #[test]
    fn adding_two_values_of_an_int64_type_gives_the_sum() {
        // Arrange
        let a = TestInt::new(3);
        let b = TestInt::new(4);
        // Act
        let sum = a + b;
        // Assert
        assert_eq!(sum, TestInt::new(7));
        assert_eq!(sum.value(), 7);
    }

    #[test]
    fn value_and_debug_text_of_an_index_type_are_the_integer() {
        // Arrange
        let a = TestIndex::new(-42);
        // Act
        let value = a.value();
        let text = format!("{a:?}");
        // Assert
        assert_eq!(value, -42);
        assert_eq!(text, "-42");
        assert_eq!(format!("{a}"), "-42");
    }

    #[test]
    fn value_and_debug_text_of_an_int64_type_are_the_integer() {
        // Arrange
        let a = TestInt::new(-42_000_000_000);
        // Act
        let value = a.value();
        let text = format!("{a:?}");
        // Assert
        assert_eq!(value, -42_000_000_000);
        assert_eq!(text, "-42000000000");
        assert_eq!(format!("{a}"), "-42000000000");
    }

    #[test]
    fn adding_one_to_the_largest_index_value_panics() {
        // Arrange
        let largest = TestIndex::new(i32::MAX);
        // Act and Assert
        expect_death(
            || {
                let _ = largest + 1;
            },
            "overflow",
        );
        expect_death(
            || {
                let _ = largest + TestIndex::new(1);
            },
            "overflow",
        );
        expect_death(
            || {
                let _ = 1 + largest;
            },
            "overflow",
        );
    }

    #[test]
    fn adding_one_to_the_largest_int64_value_panics() {
        // Arrange
        let largest = TestInt::new(i64::MAX);
        // Act and Assert
        expect_death(
            || {
                let _ = largest + 1;
            },
            "overflow",
        );
        expect_death(
            || {
                let _ = largest + TestInt::new(1);
            },
            "overflow",
        );
        expect_death(
            || {
                let _ = 1 + largest;
            },
            "overflow",
        );
    }

    #[test]
    fn incrementing_the_largest_values_panics() {
        // Arrange
        let mut index = TestIndex::new(i32::MAX);
        let mut int64 = TestInt::new(i64::MAX);
        // Act and Assert
        expect_death(
            || {
                let _ = index.post_increment();
            },
            "overflow",
        );
        expect_death(
            || {
                let _ = int64.post_increment();
            },
            "overflow",
        );
        expect_death(|| index += 1, "overflow");
        expect_death(|| int64 += 1, "overflow");
    }

    #[test]
    fn decrementing_the_smallest_values_panics() {
        // Arrange
        let mut index = TestIndex::MIN;
        let mut int64 = TestInt::MIN;
        // Act and Assert
        expect_death(
            || {
                let _ = index.post_decrement();
            },
            "overflow",
        );
        expect_death(
            || {
                let _ = int64.pre_decrement();
            },
            "overflow",
        );
        expect_death(|| index -= 1, "overflow");
        expect_death(|| int64 -= 1, "overflow");
    }

    #[test]
    fn subtracting_past_the_smallest_values_panics() {
        // Arrange
        let index = TestIndex::MIN;
        let int64 = TestInt::MIN;
        // Act and Assert
        expect_death(
            || {
                let _ = index - 1;
            },
            "overflow",
        );
        expect_death(
            || {
                let _ = int64 - TestInt::new(1);
            },
            "overflow",
        );
    }

    #[test]
    fn multiplying_past_the_largest_values_panics() {
        // Arrange
        let index = TestIndex::MAX;
        let int64 = TestInt::MAX;
        // Act and Assert
        expect_death(
            || {
                let _ = index * 2;
            },
            "overflow",
        );
        expect_death(
            || {
                let _ = int64 * TestInt::new(2);
            },
            "overflow",
        );
    }

    #[test]
    fn dividing_the_smallest_int64_value_by_minus_one_panics() {
        // Arrange
        let smallest = TestInt::MIN;
        // Act and Assert
        expect_death(
            || {
                let _ = smallest / -1;
            },
            "overflow",
        );
    }

    #[test]
    fn shifting_left_keeps_the_bits_that_fit() {
        // Arrange
        let largest = TestInt::MAX;
        // Act
        let shifted = largest << 1;
        // Assert
        assert_eq!(shifted, TestInt::new(-2));
    }

    #[test]
    fn negating_the_smallest_int64_value_panics() {
        // Arrange
        let smallest = TestInt::MIN;
        // Act and Assert
        expect_death(
            || {
                let _ = -smallest;
            },
            "overflow",
        );
    }

    #[test]
    fn hash_of_an_index_value_equals_the_hash_of_its_integer() {
        // Arrange
        let a = TestIndex::new(17);
        let b = TestIndex::new(17);
        // Act
        let hash = hash_of(&a);
        // Assert
        assert_eq!(hash, hash_of(&17_i32));
        assert_eq!(hash, hash_of(&b));
        assert_eq!(a, b);
    }

    #[test]
    fn hash_of_an_int64_value_equals_the_hash_of_its_integer() {
        // Arrange
        let a = TestInt::new(-17_000_000_000);
        let b = TestInt::new(-17_000_000_000);
        // Act
        let hash = hash_of(&a);
        // Assert
        assert_eq!(hash, hash_of(&-17_000_000_000_i64));
        assert_eq!(hash, hash_of(&b));
        assert_eq!(a, b);
    }

    #[test]
    fn ordering_of_index_values_agrees_with_their_integers() {
        // Arrange
        let a = TestIndex::new(1);
        let b = TestIndex::new(2);
        // Act and Assert
        assert!(a < b && a <= b && b > a && b >= a);
        assert!(a != b && a <= a && a >= a);
        assert!(a < 2);
        assert!(1 < b);
        assert!(a <= 1);
        assert!(2 <= b);
        assert!(2 > a);
        assert!(1 >= a);
        assert!(b >= 2);
        assert!(3 > b);
        assert!(a == 1 && 1 == a && a != 2 && 2 != a);
        assert_eq!(a.cmp(&b), 1.cmp(&2));
    }

    #[test]
    fn ordering_of_int64_values_agrees_with_their_integers() {
        // Arrange
        let a = TestInt::new(1 << 40);
        let b = TestInt::new((1 << 40) + 1);
        // Act and Assert
        assert!(a < b && a <= b && b > a && b >= a);
        assert!(a != b && a <= a && a >= a);
        assert!(a < (1 << 40) + 1);
        assert!(1 << 40 < b);
        assert!(a == 1 << 40 && 1 << 40 == a);
        assert_eq!(a.cmp(&b), (1_i64 << 40).cmp(&((1 << 40) + 1)));
    }

    #[test]
    fn index_operators_take_the_type_or_its_integer_on_either_side() {
        // Arrange
        let a = TestIndex::new(7);
        let b = TestIndex::new(3);
        // Act and Assert
        assert_eq!(a - b, TestIndex::new(4));
        assert_eq!(a - 3, TestIndex::new(4));
        assert_eq!(10 - a, TestIndex::new(3));
        assert_eq!(a * b, TestIndex::new(21));
        assert_eq!(2 * a, TestIndex::new(14));
        assert_eq!(a % b, TestIndex::new(1));
        assert_eq!(a % 4, TestIndex::new(3));
        assert_eq!(9 % a, TestIndex::new(2));
        assert_eq!(-a, TestIndex::new(-7));
        assert_eq!(a.unary_plus(), a);
        assert_eq!(i32::from(a), 7);
    }

    #[test]
    fn int64_operators_take_the_type_or_its_integer_on_either_side() {
        // Arrange
        let a = TestInt::new(40);
        let b = TestInt::new(6);
        // Act and Assert
        assert_eq!(a - b, TestInt::new(34));
        assert_eq!(a * 2, TestInt::new(80));
        assert_eq!(a / b, TestInt::new(6));
        assert_eq!(200 / a, TestInt::new(5));
        assert_eq!(a % b, TestInt::new(4));
        assert_eq!(a << 2, TestInt::new(160));
        assert_eq!(a << b, TestInt::new(40 << 6));
        assert_eq!(1 << b, TestInt::new(64));
        assert_eq!(a >> 3, TestInt::new(5));
        assert_eq!(!a, TestInt::new(!40));
        assert_eq!(-a, TestInt::new(-40));
        assert_eq!(a.unary_plus(), a);
        assert_eq!(i64::from(a), 40);
    }

    #[test]
    fn assignment_operators_take_the_type_or_its_integer() {
        // Arrange
        let mut index = TestIndex::new(10);
        let mut int64 = TestInt::new(10);
        // Act
        index += 5;
        index -= TestIndex::new(3);
        int64 += TestInt::new(6);
        int64 -= 2;
        int64 *= 3;
        int64 /= TestInt::new(7);
        int64 <<= 4;
        int64 >>= TestInt::new(1);
        int64 %= 10;
        // Assert
        assert_eq!(index, TestIndex::new(12));
        assert_eq!(
            int64,
            TestInt::new(((((10 + 6 - 2) * 3 / 7) << 4) >> 1) % 10)
        );
    }

    #[test]
    fn increment_and_decrement_return_the_new_or_the_old_value() {
        // Arrange
        let mut a = TestIndex::new(5);
        let mut b = TestInt::new(5);
        // Act and Assert
        assert_eq!(a.post_increment(), TestIndex::new(5));
        assert_eq!(*a.pre_increment(), TestIndex::new(7));
        assert_eq!(a.post_decrement(), TestIndex::new(7));
        assert_eq!(*a.pre_decrement(), TestIndex::new(5));
        assert_eq!(b.post_increment(), TestInt::new(5));
        assert_eq!(*b.pre_increment(), TestInt::new(7));
        assert_eq!(b.post_decrement(), TestInt::new(7));
        assert_eq!(*b.pre_decrement(), TestInt::new(5));
    }

    #[test]
    fn default_value_is_zero_and_limits_are_those_of_the_integer() {
        // Arrange and Act
        let index = TestIndex::default();
        let int64 = TestInt::default();
        // Assert
        assert_eq!(index.value(), 0);
        assert_eq!(int64.value(), 0);
        assert_eq!(TestIndex::MIN.value(), i32::MIN);
        assert_eq!(TestIndex::MAX.value(), i32::MAX);
        assert_eq!(TestInt::MIN.value(), i64::MIN);
        assert_eq!(TestInt::MAX.value(), i64::MAX);
    }

    #[test]
    fn type_name_is_the_name_given_to_the_macro() {
        // Arrange and Act and Assert
        assert_eq!(TestIndex::type_name(), "TestIndex");
        assert_eq!(OtherIndex::type_name(), "OtherIndex");
        assert_eq!(TestInt::type_name(), "TestInt");
        assert_eq!(OtherInt::type_name(), "OtherInt");
    }

    #[test]
    fn mutable_value_changes_the_stored_integer() {
        // Arrange
        let mut a = TestInt::new(1);
        // Act
        *a.mutable_value() = 99;
        // Assert
        assert_eq!(a, TestInt::new(99));
    }

    #[test]
    fn safe_add_into_accepts_a_type_made_by_the_macro() {
        // Arrange
        let mut b = TestInt::new(5);
        let mut at_max = TestInt::MAX;
        // Act
        let added = safe_add_into(TestInt::new(2), &mut b);
        let overflowed = safe_add_into(TestInt::new(1), &mut at_max);
        // Assert
        assert!(added);
        assert_eq!(b, TestInt::new(7));
        assert!(!overflowed);
        assert_eq!(at_max, TestInt::MAX);
    }
}
