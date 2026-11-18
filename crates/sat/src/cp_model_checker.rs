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
// Ported from ortools/sat/cp_model_checker.h at commit c931f5f7. Changed: translated from C++ to Rust.
// Ported from ortools/sat/cp_model_checker.cc at commit c931f5f7. Changed: translated from C++ to Rust.

use cpsat_proto::{CpModelProto, SatParameters};

pub fn validate_cp_model(_model: &CpModelProto, _after_presolve: bool) -> String {
    unimplemented!("phase 1: sat/cp_model_checker.cc")
}

pub fn validate_input_cp_model(_params: &SatParameters, _model: &CpModelProto) -> String {
    unimplemented!("phase 1: sat/cp_model_checker.cc")
}

pub fn solution_is_feasible(
    _model: &CpModelProto,
    _variable_values: &[i64],
    _mapping_proto: Option<&CpModelProto>,
    _postsolve_mapping: Option<&[i32]>,
) -> bool {
    unimplemented!("phase 1: sat/cp_model_checker.cc")
}

pub fn solution_can_be_optimal(_model: &CpModelProto, _variable_values: &[i64]) -> bool {
    unimplemented!("phase 1: sat/cp_model_checker.cc")
}

#[cfg(test)]
mod tests {
    use cpsat_proto::CpModelProto;
    use cpsat_testutil::parse_test_proto;

    use super::*;

    // `HasSubstr` of gMock.
    fn assert_has_substr(actual: &str, expected: &str) {
        assert!(
            actual.contains(expected),
            "{actual:?} does not contain {expected:?}"
        );
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn solution_is_feasible_test_basic_example() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { name: 'x' domain: 0 domain: 10 }
    variables { name: 'y' domain: 0 domain: 10 }
    constraints {
      linear { vars: 0 coeffs: 1 vars: 1 coeffs: 1 domain: 0 domain: 10 }
    }
  "#,
        );
        // Act and Assert
        assert!(!solution_is_feasible(&model, &[8, 8], None, None));
        assert!(!solution_is_feasible(&model, &[11, -1], None, None));
        assert!(solution_is_feasible(&model, &[5, 5], None, None));
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn solution_is_feasible_test_lin_max() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { name: 'x' domain: 0 domain: 10 }
    variables { name: 'y' domain: 0 domain: 30 }
    constraints {
      lin_max {
        target { vars: 1 coeffs: 1 offset: 0 }
        exprs { vars: 0 coeffs: 2 offset: 1 }
        exprs { vars: 0 coeffs: 3 offset: -5 }
      }
    }
  "#,
        );
        // Act and Assert
        assert!(!solution_is_feasible(&model, &[2, 4], None, None));
        assert!(!solution_is_feasible(&model, &[11, -1], None, None));
        assert!(solution_is_feasible(&model, &[2, 5], None, None));
        assert!(solution_is_feasible(&model, &[8, 19], None, None));
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn solution_is_feasible_test_or_tools_issue3769() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 1, 2 ] }
    variables { domain: [ 0, 1 ] }
    constraints {
      no_overlap_2d {
        x_intervals: [ 1, 2 ]
        y_intervals: [ 3, 4 ]
      }
    }
    constraints {
      interval {
        start { offset: 2 }
        end {
          vars: [ 1 ]
          coeffs: [ 1 ]
          offset: 2
        }
        size {
          vars: [ 1 ]
          coeffs: [ 1 ]
        }
      }
    }
    constraints {
      interval {
        start { offset: 1 }
        end { offset: 3 }
        size { offset: 2 }
      }
    }
    constraints {
      interval {
        start { offset: 1 }
        end {
          vars: [ 0 ]
          coeffs: [ 1 ]
          offset: 1
        }
        size {
          vars: [ 0 ]
          coeffs: [ 1 ]
        }
      }
    }
    constraints {
      interval {
        start { offset: 2 }
        end { offset: 2 }
        size {}
      }
    }
  "#,
        );
        // Act and Assert
        assert!(solution_is_feasible(&model, &[1, 0], None, None));
        assert!(solution_is_feasible(&model, &[1, 1], None, None));
        assert!(!solution_is_feasible(&model, &[2, 0], None, None));
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn solution_is_feasible_test_legacy_inverse() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 2 ] }
    variables { domain: [ 0, 2 ] }
    variables { domain: [ 0, 2 ] }
    variables { domain: [ 0, 2 ] }
    variables { domain: [ 0, 2 ] }
    variables { domain: [ 0, 2 ] }
    constraints {
      inverse {
        f_direct: 0
        f_direct: 1
        f_direct: 2
        f_inverse: 3
        f_inverse: 4
        f_inverse: 5
      }
    }
  "#,
        );
        // Act and Assert
        assert!(solution_is_feasible(
            &model,
            &[2, 0, 1, 1, 2, 0],
            None,
            None
        ));
        assert!(!solution_is_feasible(
            &model,
            &[2, 0, 1, 1, 0, 2],
            None,
            None
        ));
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn solution_is_feasible_test_inverse() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 2 ] }
    variables { domain: [ 0, 2 ] }
    variables { domain: [ 0, 2 ] }
    constraints {
      inverse {
        f_expr_direct: { vars: 0 coeffs: -1 offset: 3 }
        f_expr_direct: { vars: 1 coeffs: 1 offset: -1 }
        f_expr_direct: { vars: 2 coeffs: 3 offset: -2 }
        f_expr_inverse: { vars: 0 coeffs: 1 offset: 0 }
        f_expr_inverse: { vars: 1 coeffs: 1 offset: 1 }
        f_expr_inverse: { vars: 2 coeffs: -2 offset: 2 }
      }
    }
  "#,
        );
        // Act and Assert
        assert!(solution_is_feasible(&model, &[1, 1, 1], None, None));
        assert!(!solution_is_feasible(&model, &[1, 0, 2], None, None));
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn solution_is_feasible_test_reservoir() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 2 ] }
    variables { domain: [ 0, 2 ] }
    variables { domain: [ 1, 1 ] }
    variables { domain: [ 0, 1 ] }
    constraints {
      reservoir {
        time_exprs: { vars: 0 coeffs: 1 }
        time_exprs: { vars: 1 coeffs: 1 }
        level_changes: { offset: -1 }
        level_changes: { offset: 1 }
        active_literals: [ 2, 3 ]
        min_level: 0
        max_level: 2
      }
    }
  "#,
        );
        // Act and Assert
        assert!(!solution_is_feasible(&model, &[0, 0, 1, 0], None, None));
        assert!(solution_is_feasible(&model, &[0, 0, 1, 1], None, None));
        assert!(solution_is_feasible(&model, &[1, 0, 1, 1], None, None));
        assert!(!solution_is_feasible(&model, &[0, 1, 1, 1], None, None));
        assert!(!solution_is_feasible(&model, &[0, 0, 1, 0], None, None));
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn solution_is_feasible_test_reservoir_with_negative_time() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ -2, 2 ] }
    variables { domain: [ -2, 2 ] }
    constraints {
      reservoir {
        time_exprs: { vars: 0 coeffs: 1 }
        time_exprs: { vars: 1 coeffs: 1 }
        level_changes: { offset: 2 }
        level_changes: { offset: -2 }
        min_level: 0
        max_level: 2
      }
    }
  "#,
        );
        // Act and Assert
        assert!(solution_is_feasible(&model, &[1, 1], None, None));
        assert!(solution_is_feasible(&model, &[0, 0], None, None));
        assert!(!solution_is_feasible(&model, &[1, 0], None, None));
        assert!(solution_is_feasible(&model, &[0, 1], None, None));
        assert!(solution_is_feasible(&model, &[-2, 2], None, None));
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn solution_is_feasible_test_self_arc_are_ok() {
        // The literal -1 is the negation of the first variable.
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    constraints {
      circuit {
        literals: [ -1, 1, 2, 3, 0 ]
        tails: [ 0, 1, 2, 3, 0 ]
        heads: [ 0, 2, 3, 1, 2 ]
      }
    }
  "#,
        );
        // Act and Assert
        assert!(solution_is_feasible(&model, &[0, 1, 1, 1], None, None));
        assert!(!solution_is_feasible(&model, &[1, 1, 1, 1], None, None));
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn solution_is_feasible_test_sparse_circuit() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    constraints {
      circuit {
        literals: [ 0, 1, 2, 3 ]
        tails: [ -10, 10, 9, 1000 ]
        heads: [ 10, 9, 1000, -10 ]
      }
    }
  "#,
        );
        // Act and Assert
        assert!(solution_is_feasible(&model, &[1, 1, 1, 1], None, None));
        assert!(!solution_is_feasible(&model, &[1, 0, 1, 1], None, None));
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn solution_is_feasible_test_bool_xor() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    constraints { bool_xor { literals: [ 0, 1, 2, 3 ] } }
  "#,
        );
        // Act and Assert
        assert!(solution_is_feasible(&model, &[1, 0, 0, 0], None, None));
        assert!(solution_is_feasible(&model, &[1, 1, 1, 0], None, None));
        assert!(!solution_is_feasible(&model, &[1, 1, 1, 1], None, None));
        assert!(!solution_is_feasible(&model, &[1, 0, 1, 0], None, None));
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn solution_is_feasible_test_with_enforcement() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { name: 'a' domain: 0 domain: 1 }
    variables { name: 'b' domain: 0 domain: 1 }
    variables { name: 'y' domain: 0 domain: 10 }
    constraints {
      enforcement_literal: [ 0, 1 ]
      linear { vars: 2 coeffs: 1 domain: 7 domain: 7 }
    }
  "#,
        );
        // Act and Assert
        assert!(solution_is_feasible(&model, &[0, 0, 5], None, None));
        assert!(solution_is_feasible(&model, &[0, 1, 5], None, None));
        assert!(solution_is_feasible(&model, &[1, 0, 5], None, None));
        assert!(!solution_is_feasible(&model, &[1, 1, 5], None, None));
        assert!(solution_is_feasible(&model, &[1, 1, 7], None, None));
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn solution_is_feasible_test_objective_domain() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { name: 'x' domain: 0 domain: 10 }
    variables { name: 'y' domain: 0 domain: 10 }
    objective {
      vars: [ 0, 1 ]
      coeffs: [ 1, 1 ]
      domain: [ 5, 15 ]
    }
  "#,
        );
        // Act and Assert
        assert!(!solution_is_feasible(&model, &[8, 8], None, None));
        assert!(solution_is_feasible(&model, &[5, 5], None, None));
        assert!(!solution_is_feasible(&model, &[0, 0], None, None));
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_bad_variable_domain1() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { name: 'a' domain: 0 domain: 1 domain: 3 }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model, false), "odd domain");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_variable_upper_bound_too_large() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables {
      name: 'a'
      domain: [ 0, 9223372036854775807 ]
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model, false), "does not fall in");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_variable_lower_bound_too_large1() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables {
      name: 'a'
      domain: [ -9223372036854775807, 0 ]
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model, false), "does not fall in");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_variable_lower_bound_too_large2() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables {
      name: 'a'
      domain: [ -9223372036854775808, 0 ]
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model, false), "does not fall in");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_variable_domain_overflow() {
        // Arrange
        assert_eq!(i64::MAX / 2, 4611686018427387903);
        let model_ok: CpModelProto = parse_test_proto(
            r#"
    variables {
      name: 'a'
      domain: -4611686018427387903
      domain: 4611686018427387903
    }
  "#,
        );
        // Act and Assert
        assert!(validate_cp_model(&model_ok, false).is_empty());
        // Arrange
        let model_bad0: CpModelProto = parse_test_proto(
            r#"
    variables { name: 'a' domain: 0 domain: 4611686018427387904 }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model_bad0, false), "does not fall in");
        // Arrange
        let model_bad1: CpModelProto = parse_test_proto(
            r#"
    variables { name: 'a' domain: -4611686018427387904 domain: 0 }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model_bad1, false), "does not fall in");
        // Arrange
        assert_eq!(i64::MIN + 2, -9223372036854775806);
        let model_bad2: CpModelProto = parse_test_proto(
            r#"
    variables { name: 'a' domain: -9223372036854775806 domain: 2 }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model_bad2, false), "does not fall in");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_objective_overflow() {
        // Arrange
        assert_eq!(i64::MAX / 4, 2305843009213693951);
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ -2305843009213693951, 2305843009213693951 ] }
    variables { domain: [ -2305843009213693951, 2305843009213693951 ] }
    variables { domain: [ -2305843009213693951, 2305843009213693951 ] }
    objective {
      vars: [ 0, 1, 2 ]
      coeffs: [ 1, 1, 1 ]
    }
  "#,
        );
        // The min/max sum does not overflow, but their difference does.
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model, false), "overflow");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_valid_solution_hint() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    solution_hint {
      vars: [ 0, 1 ]
      values: [ 1, 2 ]
    }
  "#,
        );
        // Act and Assert
        assert!(validate_cp_model(&model, false).is_empty());
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_solution_hint1() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    solution_hint {
      vars: [ 0, 1, 2 ]
      values: [ 1, 2, 3, 4 ]
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model, false), "same size");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_solution_hint2() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    solution_hint {
      vars: [ 0, 10, 2 ]
      values: [ 1, 2, 3 ]
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model, false), "Invalid variable");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_solution_hint3() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    solution_hint {
      vars: [ 0, 2, 0 ]
      values: [ 1, 2, 3 ]
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model, false), "duplicate");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_assumptions() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    assumptions: [ 0, 1, 4 ]
  "#,
        );
        // Act and Assert
        assert_eq!(
            validate_cp_model(&model, false),
            "Invalid literal reference 4 in the 'assumptions' field."
        );
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_negative_value_in_interval_size_domain() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 0 ] }
    variables { domain: [ -7, -7, 0, 0 ] }
    constraints {
      interval {
        start { vars: 0 coeffs: 1 }
        end { vars: 1 coeffs: 1 }
        size { vars: 1 coeffs: 1 }
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(
            &validate_cp_model(&model, false),
            "The size of a performed interval must be >= 0",
        );
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_parallel_vector_must_have_the_same_size() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: 0 domain: 4503599627370529 }
    constraints {
      interval {
        start { offset: 1 }
        size { offset: 2 }
        end { offset: 3 }
      }
    }
    constraints {
      no_overlap_2d { x_intervals: 0 y_intervals: 0 y_intervals: 0 }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model, false), "must have the same size");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_invalid_domain_in_linear() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: -288230376151711744 domain: 262144 }
    variables { domain: 0 domain: 5 }
    constraints {
      linear {
        vars: [ 1, 0 ]
        coeffs: [ 1, 2 ]
        domain: [ 1, 3, 5 ]
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model, false), "Invalid domain");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_invalid_domain_in_linear2() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: -288230376151711744 domain: 262144 }
    variables { domain: 0 domain: 5 }
    constraints {
      name: "T"
      linear {
        vars: [ 1, 0 ]
        coeffs: [ 1, 2 ]
        domain: [ 3, 0 ]
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model, false), "Invalid domain");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_negated_reference_in_linear() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { name: "c" domain: 1 domain: 1 }
    variables { domain: 0 domain: 1 }
    constraints {
      int_div {
        target {}
        exprs {}
        exprs { vars: -2 coeffs: 792633495762501632 }
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(
            &validate_cp_model(&model, false),
            "Invalid negated variable",
        );
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_arity_one_in_int_prod() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 10 ] }
    constraints {
      int_prod {
        target { vars: 0 coeffs: 1 }
        exprs { vars: 1 coeffs: 1 }
      }
    }
  "#,
        );
        // Act and Assert
        assert!(validate_cp_model(&model, false).is_empty());
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_arity_three_in_int_prod() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 10 ] }
    constraints {
      int_prod {
        target { vars: 0 coeffs: 1 }
        exprs { vars: 1 coeffs: 1 }
        exprs { vars: 1 coeffs: 1 }
        exprs { vars: 1 coeffs: 1 }
      }
    }
  "#,
        );
        // Act and Assert
        assert!(validate_cp_model(&model, false).is_empty());
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_wrong_arity_in_int_div() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 10 ] }
    constraints {
      int_div {
        target { vars: 0 coeffs: 1 }
        exprs { vars: 1 coeffs: 1 }
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model, false), "have exactly 2 terms");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_divisor_domain_contains_zero() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 10 ] }
    variables { domain: [ -3, 3 ] }
    constraints {
      int_div {
        target { vars: 0 coeffs: 1 }
        exprs { vars: 1 coeffs: 1 }
        exprs { vars: 2 coeffs: 1 }
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(
            &validate_cp_model(&model, false),
            "The domain of the divisor cannot contain 0",
        );
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_divisor_spanning_across_zero() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 10 ] }
    variables { domain: [ -3, 3 ] }
    constraints {
      int_div {
        target { vars: 0 coeffs: 1 }
        exprs { vars: 1 coeffs: 1 }
        exprs { vars: 2 coeffs: 2 offset: -3 }
      }
    }
  "#,
        );
        // Act and Assert
        assert!(validate_cp_model(&model, false).is_empty());
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_two_term_divisor_never_zero() {
        // a + b with a in {-2, 2} and b in {-1, 1} is never 0, even though 0 lies
        // between its min and max.
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 10 ] }
    variables { domain: [ -2, -2, 2, 2 ] }
    variables { domain: [ -1, -1, 1, 1 ] }
    constraints {
      int_div {
        target { vars: 0 coeffs: 1 }
        exprs { vars: 1 coeffs: 1 }
        exprs { vars: 2 vars: 3 coeffs: 1 coeffs: 1 }
      }
    }
  "#,
        );
        // Act and Assert
        assert!(validate_cp_model(&model, false).is_empty());
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_two_term_divisor_can_be_zero() {
        // 2 * a + b - 1 is 0 for a = 0 and b = 1.
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 2 ] }
    variables { domain: [ 0, 1 ] }
    constraints {
      int_div {
        target { vars: 0 coeffs: 1 }
        exprs { vars: 1 coeffs: 1 }
        exprs { vars: 2 vars: 3 coeffs: 2 coeffs: 1 offset: -1 }
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(
            &validate_cp_model(&model, false),
            "The domain of the divisor cannot contain 0",
        );
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_divisor_is_zero() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 10 ] }
    constraints {
      int_div {
        target { vars: 0 coeffs: 1 }
        exprs { vars: 1 coeffs: 1 }
        exprs {}
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model, false), "Division by 0");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_wrong_arity_in_int_mod() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 10 ] }
    constraints {
      int_mod {
        target { vars: 0 coeffs: 1 }
        exprs { vars: 1 coeffs: 1 }
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model, false), "have exactly 2 terms");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_negative_modulo() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 10 ] }
    variables { domain: [ -3, 3 ] }
    constraints {
      int_mod {
        target { vars: 0 coeffs: 1 }
        exprs { vars: 1 coeffs: 1 }
        exprs { vars: 2 coeffs: 1 }
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(
            &validate_cp_model(&model, false),
            "strictly positive modulo argument",
        );
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_invalid_legacy_inverse_constraint() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: 0 domain: 1 }
    constraints { inverse { f_direct: [ 0 ] } }
  "#,
        );
        // Act and Assert
        assert_has_substr(
            &validate_cp_model(&model, false),
            "Non-matching fields size in inverse",
        );
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_invalid_inverse_constraint() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: 0 domain: 1 }
    constraints {
      inverse {
        f_expr_direct: {
          vars: [ 0 ]
          coeffs: [ 1 ]
        }
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(
            &validate_cp_model(&model, false),
            "Non-matching fields size in inverse",
        );
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_inconsistent_inverse_constraint() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: 0 domain: 1 }
    constraints {
      inverse {
        f_direct: [ 0 ]
        f_inverse: [ 0 ]
        f_expr_direct: {
          vars: [ 0 ]
          coeffs: [ 1 ]
        }
        f_expr_inverse: {
          vars: [ 0 ]
          coeffs: [ 1 ]
        }
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(
            &validate_cp_model(&model, false),
            "Inconsistent inverse with both legacy and new format defined",
        );
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_incompatible_automaton_transitions() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: 0 domain: 1 }
    constraints {
      automaton {
        final_states: 0
        transition_tail: 0
        transition_tail: 0
        transition_head: 0
        transition_head: 1
        transition_label: 0
        transition_label: 0
        vars: 0
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(
            &validate_cp_model(&model, false),
            "automaton: incompatible transitions",
        );
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_duplicate_automaton_transitions() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: 0 domain: 1 }
    constraints {
      automaton {
        final_states: 0
        transition_tail: 0
        transition_tail: 0
        transition_head: 0
        transition_head: 0
        transition_label: 0
        transition_label: 0
        vars: 0
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(
            &validate_cp_model(&model, false),
            "automaton: duplicate transition",
        );
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_interval_must_appear_before_they_are_used() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    constraints { no_overlap { intervals: [ 1, 2 ] } }
    constraints {
      interval {
        start { offset: 0 }
        end { offset: 4 }
        size { offset: 4 }
      }
    }
    constraints {
      interval {
        start { offset: 4 }
        end { offset: 5 }
        size { offset: 1 }
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(&validate_cp_model(&model, true), "must appear before");
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_valid_node_expressions() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 10 ] }
    constraints {
      routes {
        tails: [ 0, 1 ]
        heads: [ 1, 0 ]
        literals: [ 0, 1 ]
        dimensions {
          exprs {
            vars: [ 2 ]
            coeffs: [ 1 ]
          }
          exprs {
            vars: [ 3 ]
            coeffs: [ 2 ]
          }
        }
        dimensions {
          exprs {}
          exprs {}
        }
      }
    }
  "#,
        );
        // Act and Assert
        assert!(validate_cp_model(&model, false).is_empty());
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_invalid_node_expressions_count() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 10 ] }
    constraints {
      routes {
        tails: [ 0, 1 ]
        heads: [ 1, 0 ]
        literals: [ 0, 1 ]
        dimensions {
          exprs {
            vars: [ 2 ]
            coeffs: [ 1 ]
          }
          exprs {
            vars: [ 3 ]
            coeffs: [ 1 ]
          }
          exprs {
            vars: [ 2 ]
            coeffs: [ 1 ]
          }
        }
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(
            &validate_cp_model(&model, false),
            "must be of size num_nodes:2",
        );
    }

    #[test]
    #[ignore = "phase 1: sat/cp_model_checker.cc"]
    fn validate_cp_model_test_invalid_node_expression_in_routes_constraint() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 10 ] }
    constraints {
      routes {
        tails: [ 0, 1 ]
        heads: [ 1, 0 ]
        literals: [ 0, 1 ]
        dimensions {
          exprs {
            vars: [ 2 ]
            coeffs: [ 1 ]
          }
          exprs {
            vars: [ 3 ]
            coeffs: [ 1 ]
          }
        }
      }
    }
  "#,
        );
        // Act and Assert
        assert_has_substr(
            &validate_cp_model(&model, false),
            "Out of bound integer variable 3 in route constraint",
        );
    }
}
