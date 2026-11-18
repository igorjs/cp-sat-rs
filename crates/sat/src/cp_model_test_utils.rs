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
// Ported from ortools/sat/cp_model_test_utils.h at commit c931f5f7. Changed: translated from C++ to Rust.
// Ported from ortools/sat/cp_model_test_utils.cc at commit c931f5f7. Changed: translated from C++ to Rust.

use cpsat_proto::constraint_proto::Constraint;
use cpsat_proto::{
    BoolArgumentProto, ConstraintProto, CpModelProto, CpObjectiveProto, IntegerVariableProto,
    LinearConstraintProto,
};
use rand_chacha::ChaCha8Rng;
use rand_chacha::rand_core::RngCore;

use crate::cp_model_utils::negated_ref;

// Draws uniformly in [lo, hi), as absl::Uniform does for its default interval type.
fn uniform(random: &mut ChaCha8Rng, lo: i32, hi: i32) -> i32 {
    lo + (random.next_u64() % (hi - lo) as u64) as i32
}

// Draws true with probability `p`, as absl::Bernoulli does.
fn bernoulli(random: &mut ChaCha8Rng, p: f64) -> bool {
    (random.next_u64() as f64 / u64::MAX as f64) < p
}

fn boolean_variable() -> IntegerVariableProto {
    IntegerVariableProto {
        domain: vec![0, 1],
        ..Default::default()
    }
}

fn bool_or(literals: Vec<i32>) -> ConstraintProto {
    ConstraintProto {
        constraint: Some(Constraint::BoolOr(BoolArgumentProto { literals })),
        ..Default::default()
    }
}

fn linear(constraint: LinearConstraintProto) -> ConstraintProto {
    ConstraintProto {
        constraint: Some(Constraint::Linear(constraint)),
        ..Default::default()
    }
}

// Generates a pairwise bool_or pigeonhole problem placing `num_pigeons`
// pigeons into `num_holes` holes (defaulting to `num_pigeons - 1`, which is
// unsatisfiable). Variable `i * num_holes + j` indicates whether pigeon `i` is
// placed in hole `j`.
pub fn pigeonhole_problem(num_pigeons: i32, num_holes: Option<i32>) -> CpModelProto {
    let holes = num_holes.unwrap_or(num_pigeons - 1);
    let mut result = CpModelProto {
        name: "Pigeonhole problem".to_string(),
        ..Default::default()
    };
    for _ in 0..num_pigeons * holes {
        result.variables.push(boolean_variable());
    }
    // Each pigeon is in at least one hole.
    for i in 0..num_pigeons {
        result
            .constraints
            .push(bool_or((0..holes).map(|j| i * holes + j).collect()));
    }
    // Each hole contains at most one pigeon.
    for j in 0..holes {
        for i1 in 0..num_pigeons {
            for i2 in (i1 + 1)..num_pigeons {
                result.constraints.push(bool_or(vec![
                    negated_ref(i1 * holes + j),
                    negated_ref(i2 * holes + j),
                ]));
            }
        }
    }
    result
}

// Generates a random 3-SAT problem with a number of constraints given by:
// num_variables * proportion_of_constraints. With the upstream default
// proportion value, 4.26, we are around the transition SAT/UNSAT.
pub fn random_3sat_problem(
    num_variables: i32,
    proportion_of_constraints: f64,
    random: &mut ChaCha8Rng,
) -> CpModelProto {
    let mut result = CpModelProto {
        name: "Random 3-SAT".to_string(),
        ..Default::default()
    };
    for _ in 0..num_variables {
        result.variables.push(boolean_variable());
    }
    let num_constraints = (proportion_of_constraints * num_variables as f64) as i32;
    for _ in 0..num_constraints {
        let mut literals: Vec<i32> = Vec::new();
        while literals.len() != 3 {
            let literal = uniform(random, negated_ref(num_variables - 1), num_variables);
            if !literals.contains(&literal) {
                literals.push(literal);
            }
        }
        result.constraints.push(bool_or(literals));
    }
    result
}

// Generates a random 0-1 "covering" optimization linear problem:
// - Each constraint has density ~0.5 and asks for a sum >= num_variables / 10.
// - The objective is to minimize the number of variables at 1.
pub fn random_linear_problem(
    num_variables: i32,
    num_constraints: i32,
    random: &mut ChaCha8Rng,
) -> CpModelProto {
    let mut result = CpModelProto {
        name: "Random 0-1 linear problem".to_string(),
        ..Default::default()
    };
    for _ in 0..num_variables {
        result.variables.push(boolean_variable());
    }
    for _ in 0..num_constraints {
        // Sum >= num_variables / 10.
        let min_value = num_variables / 10;
        let mut ct = LinearConstraintProto {
            domain: vec![min_value as i64, i64::MAX],
            ..Default::default()
        };
        for v in 0..num_variables {
            if bernoulli(random, 0.5)
                // To ensure that the constraint is feasible, we enforce that it has
                // at least the 'minimum' number of terms. This clause should only
                // rarely be used, when num_variables is high.
                || num_variables - v <= min_value - ct.vars.len() as i32
            {
                ct.vars.push(v);
                ct.coeffs.push(1);
            }
        }
        result.constraints.push(linear(ct));
    }

    // Objective: minimize variables at one.
    {
        let objective_var_index = result.variables.len() as i32;
        result.variables.push(IntegerVariableProto {
            domain: vec![0, num_variables as i64],
            ..Default::default()
        });
        result.objective = Some(CpObjectiveProto {
            vars: vec![objective_var_index],
            coeffs: vec![1],
            ..Default::default()
        });

        // Sum of all other variables == 0
        let mut ct = LinearConstraintProto {
            domain: vec![0, 0],
            ..Default::default()
        };
        for v in 0..num_variables {
            ct.vars.push(v);
            ct.coeffs.push(1);
        }
        ct.vars.push(objective_var_index);
        ct.coeffs.push(-1);
        result.constraints.push(linear(ct));
    }

    result
}

#[cfg(test)]
mod tests {
    use cpsat_proto::constraint_proto::Constraint;
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::SeedableRng;

    use super::*;
    use crate::cp_model_checker::validate_cp_model;

    const SEED: u64 = 12345;

    fn bool_or_literals(model: &CpModelProto) -> Vec<&Vec<i32>> {
        model
            .constraints
            .iter()
            .map(|ct| match &ct.constraint {
                Some(Constraint::BoolOr(arg)) => &arg.literals,
                other => panic!("expected a bool_or, got {other:?}"),
            })
            .collect()
    }

    #[test]
    fn pigeonhole_problem_with_given_holes_has_the_pairwise_clauses() {
        // Arrange
        let (pigeons, holes) = (3, 2);

        // Act
        let model = pigeonhole_problem(pigeons, Some(holes));

        // Assert
        assert_eq!(model.variables.len(), 6);
        assert!(model.variables.iter().all(|v| v.domain == vec![0, 1]));
        let clauses = bool_or_literals(&model);
        assert_eq!(clauses.len(), 9);
        assert!(clauses.iter().all(|c| c.len() == 2));
        assert_eq!(
            clauses.iter().filter(|c| c.iter().all(|&l| l >= 0)).count(),
            3
        );
        assert_eq!(
            clauses.iter().filter(|c| c.iter().all(|&l| l < 0)).count(),
            6
        );
    }

    #[test]
    fn pigeonhole_problem_without_holes_uses_one_hole_fewer_than_pigeons() {
        // Arrange
        let pigeons = 4;

        // Act
        let model = pigeonhole_problem(pigeons, None);

        // Assert
        assert_eq!(model.variables.len(), 12);
        // One clause per pigeon, and one per pair of pigeons in each of the 3 holes.
        assert_eq!(bool_or_literals(&model).len(), 4 + 3 * 6);
    }

    #[test]
    fn random_generators_give_equal_models_for_equal_seeds() {
        // Arrange
        let mut first = ChaCha8Rng::seed_from_u64(SEED);
        let mut second = ChaCha8Rng::seed_from_u64(SEED);

        // Act
        let sat_a = random_3sat_problem(10, 4.26, &mut first);
        let sat_b = random_3sat_problem(10, 4.26, &mut second);
        let linear_a = random_linear_problem(10, 5, &mut first);
        let linear_b = random_linear_problem(10, 5, &mut second);

        // Assert
        assert_eq!(sat_a, sat_b);
        assert_eq!(linear_a, linear_b);
        let mut other = ChaCha8Rng::seed_from_u64(SEED + 1);
        assert_ne!(random_3sat_problem(10, 4.26, &mut other), sat_a);
        assert_ne!(random_linear_problem(10, 5, &mut other), linear_a);
        // 4.26 * 10 clauses, truncated, each over three distinct literals.
        let clauses = bool_or_literals(&sat_a);
        assert_eq!(clauses.len(), 42);
        for clause in clauses {
            assert_eq!(clause.len(), 3);
            assert!(clause.iter().all(|l| (-10..10).contains(l)));
            assert!(clause[0] != clause[1] && clause[0] != clause[2] && clause[1] != clause[2]);
        }
        // The 5 covering constraints and the one that defines the objective.
        assert_eq!(linear_a.constraints.len(), 6);
    }

    #[test]
    fn generated_models_pass_validate_cp_model() {
        // Arrange
        let mut random = ChaCha8Rng::seed_from_u64(SEED);

        // Act
        let models = [
            pigeonhole_problem(3, Some(2)),
            pigeonhole_problem(4, None),
            random_3sat_problem(10, 4.26, &mut random),
            random_linear_problem(10, 5, &mut random),
        ];

        // Assert
        for model in &models {
            assert_eq!(validate_cp_model(model, false), "", "{}", model.name);
        }
    }
}
