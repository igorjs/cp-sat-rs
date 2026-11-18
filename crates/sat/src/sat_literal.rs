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
// Ported from ortools/sat/sat_literal.h at commit c931f5f7. Changed: translated from C++ to Rust.

// Sat Literal (a boolean variable or its negation).

use std::fmt;

use cpsat_util::define_strong_index_type;

// Index of a variable (>= 0).
define_strong_index_type!(pub BooleanVariable);
pub const K_NO_BOOLEAN_VARIABLE: BooleanVariable = BooleanVariable::new(-1);

// Index of a literal (>= 0), see Literal below.
define_strong_index_type!(pub LiteralIndex);
pub const K_NO_LITERAL_INDEX: LiteralIndex = LiteralIndex::new(-1);

// Special values used in some API to indicate a literal that is always true
// or always false.
pub const K_TRUE_LITERAL_INDEX: LiteralIndex = LiteralIndex::new(-2);
pub const K_FALSE_LITERAL_INDEX: LiteralIndex = LiteralIndex::new(-3);

// A literal is used to represent a variable or its negation. If it represents
// the variable it is said to be positive. If it represents its negation, it is
// said to be negative. We support two representations as an integer.
//
// The "signed" encoding of a literal is convenient for input/output and is used
// in the cnf file format. For a 0-based variable index x, (x + 1) represents
// the variable x and -(x + 1) represents its negation. The signed value 0 is an
// undefined literal and this class can never contain it.
//
// The "index" encoding of a literal is convenient as an index to an array
// and is the one used internally for efficiency. It is always positive or zero,
// and for a 0-based variable index x, (x << 1) encodes the variable x and the
// same number XOR 1 encodes its negation.
//
// `Default` is the literal of index 0: the C++ default constructor leaves the index unset.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Literal {
    index: i32,
}

impl Literal {
    pub const fn new_signed_value(signed_value: i32) -> Literal {
        assert!(
            signed_value != 0,
            "assertion `left != right` failed: signed_value is 0"
        );
        Literal {
            index: if signed_value > 0 {
                (signed_value - 1) << 1
            } else {
                ((-signed_value - 1) << 1) ^ 1
            },
        }
    }

    pub const fn new_literal_index(index: LiteralIndex) -> Literal {
        Literal {
            index: index.value(),
        }
    }

    pub const fn new(variable: BooleanVariable, is_positive: bool) -> Literal {
        Literal {
            index: if is_positive {
                variable.value() << 1
            } else {
                (variable.value() << 1) ^ 1
            },
        }
    }

    pub const fn variable(&self) -> BooleanVariable {
        BooleanVariable::new(self.index >> 1)
    }

    pub const fn is_positive(&self) -> bool {
        (self.index & 1) == 0
    }

    pub const fn is_negative(&self) -> bool {
        (self.index & 1) != 0
    }

    pub const fn index(&self) -> LiteralIndex {
        LiteralIndex::new(self.index)
    }

    pub const fn negated_index(&self) -> LiteralIndex {
        LiteralIndex::new(self.index ^ 1)
    }

    pub const fn signed_value(&self) -> i32 {
        if (self.index & 1) != 0 {
            -((self.index >> 1) + 1)
        } else {
            (self.index >> 1) + 1
        }
    }

    pub const fn negated(&self) -> Literal {
        Literal::new_literal_index(self.negated_index())
    }

    pub fn debug_string(&self) -> String {
        if self.index == K_NO_LITERAL_INDEX.value() {
            return "NA".to_string();
        }
        format!("{:+}", self.signed_value())
    }
}

// The implicit conversion `operator LiteralIndex()` upstream.
impl From<Literal> for LiteralIndex {
    fn from(literal: Literal) -> LiteralIndex {
        literal.index()
    }
}

impl fmt::Display for Literal {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.debug_string())
    }
}

/// The `operator<<` of upstream for a span of literals: `[+1,-2]`.
pub fn format_literals(literals: &[Literal]) -> String {
    let parts: Vec<String> = literals.iter().map(Literal::debug_string).collect();
    format!("[{}]", parts.join(","))
}

/// The `operator<<` of upstream for a span of literal indices.
pub fn format_literal_indices(literals: &[LiteralIndex]) -> String {
    let parts: Vec<String> = literals
        .iter()
        .map(|index| Literal::new_literal_index(*index).debug_string())
        .collect();
    format!("[{}]", parts.join(","))
}

// Only used for testing to use the classical SAT notation for a literal. This
// allows to write Literals({+1, -4, +3}) for the clause with BooleanVariable 0
// and 2 appearing positively and 3 negatively.
pub fn literals(input: &[i32]) -> Vec<Literal> {
    input
        .iter()
        .map(|signed_value| Literal::new_signed_value(*signed_value))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpsat_testutil::expect_death;

    #[test]
    fn boolean_variable_test_api() {
        // Arrange
        let var1 = BooleanVariable::new(1);
        let var2 = BooleanVariable::new(2);
        let var3 = BooleanVariable::new(2);

        // Act and Assert
        assert_ne!(var1, var2);
        assert_eq!(var2, var3);
    }

    #[test]
    fn literal_test_api() {
        // Arrange
        let var1 = BooleanVariable::new(1);
        let var2 = BooleanVariable::new(2);
        let l1 = Literal::new(var1, true);
        let l2 = Literal::new(var2, false);

        // Act
        let l3 = l2.negated();

        // Assert
        assert_eq!(l1.variable(), var1);
        assert_eq!(l2.variable(), var2);
        assert_eq!(l3.variable(), var2);
        assert!(l1.is_positive());
        assert!(l2.is_negative());
        assert!(l3.is_positive());
    }

    #[test]
    fn signed_value_and_index_encodings_agree() {
        // Arrange
        let cases = [(1, 0), (-1, 1), (2, 2), (-2, 3), (5, 8), (-5, 9)];

        for (signed_value, index) in cases {
            // Act
            let literal = Literal::new_signed_value(signed_value);

            // Assert
            assert_eq!(literal.index(), LiteralIndex::new(index));
            assert_eq!(literal.signed_value(), signed_value);
            assert_eq!(
                Literal::new_literal_index(LiteralIndex::new(index)),
                literal
            );
            assert_eq!(literal.negated_index(), LiteralIndex::new(index ^ 1));
            assert_eq!(LiteralIndex::from(literal), literal.index());
        }
    }

    #[test]
    fn signed_value_of_zero_dies() {
        // Act and Assert
        expect_death(
            || {
                let _ = Literal::new_signed_value(0);
            },
            "assertion `left != right` failed",
        );
    }

    #[test]
    fn literals_order_by_index_and_print_with_a_sign() {
        // Arrange
        let clause = literals(&[1, -4, 3]);

        // Act
        let text = format_literals(&clause);

        // Assert
        assert_eq!(text, "[+1,-4,+3]");
        assert!(clause[0] < clause[2]);
        assert!(clause[2] <= clause[2]);
        assert_eq!(Literal::new_signed_value(-2).to_string(), "-2");
        assert_eq!(
            format_literal_indices(&[LiteralIndex::new(0), LiteralIndex::new(3)]),
            "[+1,-2]"
        );
        assert_eq!(
            Literal::new_literal_index(K_NO_LITERAL_INDEX).debug_string(),
            "NA"
        );
        assert_eq!(format_literals(&[]), "[]");
    }
}
