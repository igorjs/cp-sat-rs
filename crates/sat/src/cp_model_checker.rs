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

use std::collections::{BTreeMap, HashMap, HashSet};
use std::sync::LazyLock;
use std::sync::atomic::{AtomicBool, Ordering};

use cpsat_port::proto_utils::{protobuf_debug_string, protobuf_short_debug_string};
use cpsat_proto::constraint_proto::Constraint;
use cpsat_proto::decision_strategy_proto::{DomainReductionStrategy, VariableSelectionStrategy};
use cpsat_proto::{
    AllDifferentConstraintProto, AutomatonConstraintProto, BoolArgumentProto,
    CircuitConstraintProto, ConstraintProto, CpModelProto, CpObjectiveProto,
    CumulativeConstraintProto, ElementConstraintProto, FloatObjectiveProto, IntegerVariableProto,
    IntervalConstraintProto, InverseConstraintProto, LinearArgumentProto, LinearConstraintProto,
    LinearExpressionProto, NoOverlap2DConstraintProto, NoOverlapConstraintProto,
    ReservoirConstraintProto, RoutesConstraintProto, SatParameters, TableConstraintProto,
};
use cpsat_util::saturated_arithmetic::{at_min_or_max_int64, cap_abs, cap_add, cap_prod, cap_sub};
use cpsat_util::sorted_interval_list::Domain;

use crate::cp_model_utils::{
    ConstraintCase, ProtoWithDomain, constraint_case, constraint_case_name,
    domain_in_proto_contains, get_references_used_by_constraint, lin_max_is_canonical,
    positive_ref, read_domain_from_proto, ref_is_positive, std_gcd, used_intervals, used_variables,
};
use crate::diffn_util::{
    Rectangle, find_one_intersection_if_present, find_one_intersection_if_present_with_zero_area,
};
use crate::integer_base::IntegerValue;
use crate::primary_variables::{
    compute_all_variables_from_primary_variables, compute_variable_relationships,
};
use crate::util::diophantine_equation_of_size_two_has_solution_in_domain;

// The flag `cp_model_check_dependent_variables`: when true, check that solutions can be
// computed only from their free variables.
pub static CP_MODEL_CHECK_DEPENDENT_VARIABLES: AtomicBool = AtomicBool::new(false);

// If the string returned by "statement" is not empty, returns it.
macro_rules! return_if_not_empty {
    ($statement:expr) => {{
        let error_message: String = $statement;
        if !error_message.is_empty() {
            return error_message;
        }
    }};
}

// =============================================================================
// CpModelProto validation.
// =============================================================================

// Verifies that the given model satisfies all the properties described in the
// proto comments. Returns an empty string if it is the case, otherwise fails at
// the first error and returns a human-readable description of the issue.
//
// The extra parameter is internal and mainly for debugging. After the problem
// has been presolved, we have a stricter set of properties we want to enforce.
//
// TODO(user): Add any needed overflow validation because we are far from
// exhaustive. We could also run a small presolve that tightens variable bounds
// before the overflow check to facilitate the lives of our users, but it is
// some work to put in place.
//
// Signed overflow that upstream leaves undefined panics here, and so does the
// index of an empty repeated field, as in the `after_presolve` check of a linear
// constraint with no domain.
pub fn validate_cp_model(model: &CpModelProto, after_presolve: bool) -> String {
    let mut int128_overflow: i64 = 0;
    for v in 0..model.variables.len() {
        return_if_not_empty!(validate_integer_variable(model, v));

        let domain = &model.variables[v].domain;
        let min = domain[0];
        let max = domain[domain.len() - 1];
        int128_overflow = cap_add(
            int128_overflow,
            [min.abs(), max.abs(), max - min]
                .into_iter()
                .max()
                .expect("the array is not empty"),
        );
    }

    // We require this precondition so that we can take any linear combination of
    // variables with coefficients in int64_t and compute the activity on an
    // int128 with no overflow. This is useful during cut computation.
    if int128_overflow == i64::MAX {
        return "The sum of all variable domains does not fit on an int64_t. This \
                is needed to prevent overflows."
            .to_owned();
    }

    // We need to validate the intervals used first, so we add these constraints
    // here so that we can validate them in a second pass.
    let mut constraints_using_intervals: Vec<usize> = Vec::new();

    for c in 0..model.constraints.len() {
        return_if_not_empty!(validate_variables_used_in_constraint(model, c));

        // Other non-generic validations.
        let ct = &model.constraints[c];
        match &ct.constraint {
            Some(
                Constraint::BoolOr(_)
                | Constraint::BoolAnd(_)
                | Constraint::AtMostOne(_)
                | Constraint::ExactlyOne(_)
                | Constraint::BoolXor(_),
            ) => {}
            Some(Constraint::Linear(_)) => {
                return_if_not_empty!(validate_linear_constraint(model, ct, after_presolve));
            }
            Some(Constraint::LinMax(lin_max)) => {
                return_if_not_empty!(validate_linear_expression(model, or_empty(&lin_max.target)));
                for expr in &lin_max.exprs {
                    return_if_not_empty!(validate_linear_expression(model, expr));
                }
                if after_presolve && !lin_max_is_canonical(ct) {
                    return format!(
                        "LinMax constraint is not in canonical form: {}",
                        protobuf_short_debug_string(ct)
                    );
                }
            }
            Some(Constraint::IntProd(_)) => {
                return_if_not_empty!(validate_int_prod_constraint(model, ct));
            }
            Some(Constraint::IntDiv(_)) => {
                return_if_not_empty!(validate_int_div_constraint(model, ct));
            }
            Some(Constraint::IntMod(_)) => {
                return_if_not_empty!(validate_int_mod_constraint(model, ct));
            }
            Some(Constraint::Inverse(_)) => {
                return_if_not_empty!(validate_inverse_constraint(model, ct));
            }
            Some(Constraint::AllDiff(all_diff)) => {
                for expr in &all_diff.exprs {
                    return_if_not_empty!(validate_linear_expression(model, expr));
                }
            }
            Some(Constraint::Element(_)) => {
                return_if_not_empty!(validate_element_constraint(model, ct));
            }
            Some(Constraint::Table(_)) => {
                return_if_not_empty!(validate_table_constraint(model, ct));
            }
            Some(Constraint::Automaton(_)) => {
                return_if_not_empty!(validate_automaton_constraint(model, ct));
            }
            Some(Constraint::Circuit(circuit)) => {
                return_if_not_empty!(validate_graph_input(false, circuit));
            }
            Some(Constraint::Routes(_)) => {
                return_if_not_empty!(validate_routes_constraint(model, ct));
            }
            Some(Constraint::Interval(_)) => {
                return_if_not_empty!(validate_interval_constraint(model, ct));
            }
            Some(Constraint::Cumulative(_)) => constraints_using_intervals.push(c),
            Some(Constraint::NoOverlap(_)) => constraints_using_intervals.push(c),
            Some(Constraint::NoOverlap2d(_)) => constraints_using_intervals.push(c),
            Some(Constraint::Reservoir(_)) => {
                return_if_not_empty!(validate_reservoir_constraint(model, ct));
            }
            Some(Constraint::DummyConstraint(_)) => {
                return "The dummy constraint should never appear in a model.".to_owned();
            }
            None => {}
        }
    }

    // Extra validation for constraints using intervals.
    for &c in &constraints_using_intervals {
        return_if_not_empty!(validate_intervals_used_in_constraint(
            after_presolve,
            model,
            c
        ));

        let ct = &model.constraints[c];
        match &ct.constraint {
            Some(Constraint::Cumulative(_)) => {
                return_if_not_empty!(validate_cumulative_constraint(model, ct));
            }
            Some(Constraint::NoOverlap(_)) => {}
            Some(Constraint::NoOverlap2d(_)) => {
                return_if_not_empty!(validate_no_overlap_2d_constraint(model, ct));
            }
            _ => {
                if cfg!(debug_assertions) {
                    panic!("Shouldn't be here");
                }
                log::error!("Shouldn't be here");
            }
        }
    }

    if !after_presolve && model.objective.is_some() && model.floating_point_objective.is_some() {
        return "A model cannot have both an objective and a floating point \
                objective."
            .to_owned();
    }
    if let Some(objective) = &model.objective {
        if objective.scaling_factor != 0.0 && !objective.scaling_factor.is_normal() {
            return "A model cannot have an objective with a nan, inf or subnormal \
                    scaling factor"
                .to_owned();
        }

        return_if_not_empty!(validate_objective(model, objective));

        if objective.integer_scaling_factor != 0
            || objective.integer_before_offset != 0
            || objective.integer_after_offset != 0
        {
            // If any of these fields are set, the domain must be set.
            if objective.domain.is_empty() {
                return "Objective integer scaling or offset is set without an objective \
                        domain."
                    .to_owned();
            }

            // Check that we can transform any value in the objective domain without
            // overflow. We only check the bounds which is enough.
            let mut overflow = false;
            for &v in &objective.domain {
                let mut t = cap_add(v, objective.integer_before_offset);
                if at_min_or_max_int64(t) {
                    overflow = true;
                    break;
                }
                t = cap_prod(t, objective.integer_scaling_factor);
                if at_min_or_max_int64(t) {
                    overflow = true;
                    break;
                }
                t = cap_add(t, objective.integer_after_offset);
                if at_min_or_max_int64(t) {
                    overflow = true;
                    break;
                }
            }
            if overflow {
                return format!(
                    "Internal fields related to the postsolve of the integer objective \
                     are causing a potential integer overflow: {}",
                    protobuf_short_debug_string(objective)
                );
            }
        }
    }
    return_if_not_empty!(validate_search_strategies(model));
    return_if_not_empty!(validate_solution_hint(model));
    for &r in &model.assumptions {
        if !literal_reference_is_valid(model, r) {
            return format!("Invalid literal reference {r} in the 'assumptions' field.");
        }
    }
    String::new()
}

// Some validation (in particular the floating point objective) requires reading
// parameters.
//
// TODO(user): Ideally we would have just one ValidateCpModel() function but
// this was introduced after many users already used ValidateCpModel() without
// parameters.
pub fn validate_input_cp_model(params: &SatParameters, model: &CpModelProto) -> String {
    return_if_not_empty!(validate_cp_model(model, false));
    if let Some(floating_point_objective) = &model.floating_point_objective {
        return_if_not_empty!(validate_floating_point_objective(
            params.mip_max_valid_magnitude(),
            model,
            floating_point_objective
        ));
    }
    String::new()
}

// This is the logic used by PossibleIntegerOverflow().
// It is exposed so that we can reuse it in more situations.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct LinearOverflowChecker {
    pub sum_min: i64,
    pub sum_max: i64,
}

impl LinearOverflowChecker {
    // Returns false if adding coeff * X for X in [min_domain, max_domain] to a
    // linear equation does not satisfy our overflow preconditions.
    pub fn add_term(&mut self, coeff: i64, min_domain: i64, max_domain: i64) -> bool {
        if coeff == i64::MIN {
            return false;
        }
        let prod1 = cap_prod(min_domain, coeff);
        let prod2 = cap_prod(max_domain, coeff);
        if at_min_or_max_int64(prod1) {
            return false;
        }
        if at_min_or_max_int64(prod2) {
            return false;
        }

        // Note that we use min/max with zero to disallow "alternative" terms and
        // be sure that we cannot have an overflow if we do the computation in a
        // different order.
        self.sum_min = cap_add(self.sum_min, 0.min(prod1.min(prod2)));
        self.sum_max = cap_add(self.sum_max, 0.max(prod1.max(prod2)));

        // In addition to computing the min/max possible sum, we also often compare
        // it with the constraint bounds, so we do not want max - min to overflow.
        // We might also create an intermediate variable to represent the sum.
        //
        // Note that it is important to be symmetric here, as we do not want expr to
        // pass but not -expr!
        self.sum_min >= -i64::MAX / 2 && self.sum_max <= i64::MAX / 2
    }
}

// Checks if a given linear expression can create overflow. If it doesn't, sets
// `implied_domain` to a superset of the implied domain of the expression.
// This is exposed to test new constraints created during the presolve.
pub fn possible_integer_overflow(
    model: &CpModelProto,
    vars: &[i32],
    coeffs: &[i64],
    offset: i64,
    implied_domain: Option<&mut (i64, i64)>,
) -> bool {
    if offset < i64::MIN / 2 {
        return true;
    }
    if offset > i64::MAX / 2 {
        return true;
    }
    let offset = offset.abs();

    let mut checker = LinearOverflowChecker {
        sum_min: -offset.abs(),
        sum_max: offset.abs(),
    };
    for i in 0..vars.len() {
        let r#ref = vars[i];
        assert!(ref_is_positive(r#ref));
        let var_proto = &model.variables[r#ref as usize];
        let min_domain = var_proto.domain[0];
        let max_domain = var_proto.domain[var_proto.domain.len() - 1];

        if !checker.add_term(coeffs[i], min_domain, max_domain) {
            return true;
        }
    }

    if let Some(implied_domain) = implied_domain {
        *implied_domain = (checker.sum_min, checker.sum_max);
    }
    false
}

// Verifies that the given variable assignment is a feasible solution of the
// given model. The values vector should be in one-to-one correspondence with
// the model.variables() list of variables.
//
// The last two arguments are optional and help debug a failing constraint due
// to presolve.
//
// Signed overflow that upstream leaves undefined panics here, and so does the
// index of a variable or an interval that the model does not hold. A division
// by zero in an int_div or an int_mod constraint panics where upstream stops
// the process.
pub fn solution_is_feasible(
    model: &CpModelProto,
    variable_values: &[i64],
    mapping_proto: Option<&CpModelProto>,
    postsolve_mapping: Option<&[i32]>,
) -> bool {
    if variable_values.len() != model.variables.len() {
        log::debug!(
            "Wrong number of variables ({}) in the solution vector. It should be {}.",
            variable_values.len(),
            model.variables.len()
        );
        return false;
    }

    // Check that all values fall in the variable domains.
    for (i, variable) in model.variables.iter().enumerate() {
        if !domain_in_proto_contains(variable, variable_values[i]) {
            log::debug!(
                "Variable #{i} has value {} which does not fall in its domain: {}",
                variable_values[i],
                protobuf_short_debug_string(variable)
            );
            return false;
        }
    }

    assert_eq!(variable_values.len(), model.variables.len());
    let checker = ConstraintChecker::new(variable_values);

    for c in 0..model.constraints.len() {
        let ct = &model.constraints[c];
        if checker.constraint_is_feasible(model, ct) {
            continue;
        }

        // Display a message to help debugging.
        log::debug!(
            "Failing constraint #{c} : {}",
            protobuf_short_debug_string(&model.constraints[c])
        );
        if let (Some(mapping_proto), Some(postsolve_mapping)) = (mapping_proto, postsolve_mapping) {
            let mut reverse_map: Vec<i32> = vec![-1; mapping_proto.variables.len()];
            for var in 0..postsolve_mapping.len() {
                reverse_map[postsolve_mapping[var] as usize] = var as i32;
            }
            for var in used_variables(&model.constraints[c]) {
                log::debug!(
                    "var: {var} mapped_to: {} value: {} initial_domain: {} postsolved_domain: {}",
                    reverse_map[var as usize],
                    variable_values[var as usize],
                    read_domain_from_proto(&model.variables[var as usize]),
                    read_domain_from_proto(&mapping_proto.variables[var as usize])
                );
            }
        } else {
            for var in used_variables(&model.constraints[c]) {
                log::debug!("var: {var} value: {}", variable_values[var as usize]);
            }
        }
        return false;
    }

    // Check that the objective is within its domain.
    //
    // TODO(user): This is not really a "feasibility" question, but we should
    // probably check that the response objective matches with the one we can
    // compute here. This might better be done in another function though.
    if let Some(objective) = &model.objective {
        let mut inner_objective: i64 = 0;
        let num_variables = objective.coeffs.len();
        for i in 0..num_variables {
            inner_objective += checker.value(objective.vars[i]) * objective.coeffs[i];
        }
        if !objective.domain.is_empty() && !domain_in_proto_contains(objective, inner_objective) {
            log::debug!(
                "Objective value {inner_objective} not in domain! {}",
                read_domain_from_proto(objective)
            );
            return false;
        }
        let mut factor = objective.scaling_factor;
        if factor == 0.0 {
            factor = 1.0;
        }
        let scaled_objective = factor * (inner_objective as f64 + objective.offset);
        log::trace!("Checker inner objective = {inner_objective}");
        log::trace!("Checker scaled objective = {scaled_objective}");
    }

    true
}

// Verifies some invariants that any optimal solution must satisfy.
pub fn solution_can_be_optimal(model: &CpModelProto, variable_values: &[i64]) -> bool {
    if CP_MODEL_CHECK_DEPENDENT_VARIABLES.load(Ordering::Relaxed) {
        let relationships = compute_variable_relationships(model);
        let mut all_variables: Vec<i64> = variable_values.to_vec();
        for &var in &relationships.secondary_variables {
            all_variables[var as usize] = -999999; // Those values should be overwritten.
        }
        assert!(compute_all_variables_from_primary_variables(
            model,
            &relationships,
            &mut all_variables
        ));
        assert!(all_variables == variable_values);
    }
    true
}

// Checks a single constraint for feasibility.
// This has some overhead, and should only be used for debugging.
// The full model is needed for scheduling constraints that refer to intervals.
pub fn constraint_is_feasible(
    model: &CpModelProto,
    constraint: &ConstraintProto,
    variable_values: &[i64],
) -> bool {
    let checker = ConstraintChecker::new(variable_values);
    checker.constraint_is_feasible(model, constraint)
}

// The C++ getter of an unset message field returns the empty default message.
fn or_empty(expr: &Option<LinearExpressionProto>) -> &LinearExpressionProto {
    static EMPTY: LazyLock<LinearExpressionProto> = LazyLock::new(LinearExpressionProto::default);
    expr.as_ref().unwrap_or(&EMPTY)
}

// `ProtobufDebugString(message)` with the default limits.
fn debug_string<P: prost::Message + prost::Name + Default>(message: &P) -> String {
    protobuf_debug_string(message, i32::MAX, i32::MAX)
}

// `absl::StrCat(double)`: six significant digits, as `%g` prints them.
fn six_digits_to_string(value: f64) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    let sign = if value.is_sign_negative() { "-" } else { "" };
    if value == 0.0 {
        return format!("{sign}0");
    }
    if value.is_infinite() {
        return format!("{sign}inf");
    }
    let magnitude = value.abs();
    let scientific = format!("{magnitude:.5e}");
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("a float in scientific notation has an exponent");
    let exponent: i32 = exponent.parse().expect("the exponent is an integer");
    let trim = |digits: &str| -> String {
        if digits.contains('.') {
            digits
                .trim_end_matches('0')
                .trim_end_matches('.')
                .to_owned()
        } else {
            digits.to_owned()
        }
    };
    if !(-4..6).contains(&exponent) {
        return format!("{sign}{}e{exponent:+03}", trim(mantissa));
    }
    let decimals = (5 - exponent) as usize;
    format!("{sign}{}", trim(&format!("{magnitude:.decimals$}")))
}

fn domain_in_proto_is_valid<P: ProtoWithDomain>(proto: &P) -> bool {
    let domain = proto.domain();
    if domain.is_empty() {
        return true;
    }
    if !domain.len().is_multiple_of(2) {
        return false;
    }
    if domain[0] > domain[1] {
        return false;
    }
    for i in (2..domain.len()).step_by(2) {
        if domain[i] > domain[i + 1] {
            return false;
        }

        // Consecutive intervals must be non-adjacent.
        // We test it in two steps so the +1 cannot overflow.
        if domain[i - 1] >= domain[i] {
            return false;
        }
        if domain[i - 1] + 1 >= domain[i] {
            return false;
        }
    }
    true
}

fn variable_reference_is_valid(model: &CpModelProto, reference: i32) -> bool {
    // We do it this way to avoid overflow if reference is kint64min for instance.
    if reference >= model.variables.len() as i32 {
        return false;
    }
    reference >= -(model.variables.len() as i32)
}

// NOTE(user): Historically we always accepted positive or negative variable
// references everywhere, but now that we can always substitute affine
// relations, we start to transition to positive references only, which are
// clearer. Note that this doesn't concern literal references though.
fn variable_index_is_valid(model: &CpModelProto, var: i32) -> bool {
    var >= 0 && var < model.variables.len() as i32
}

fn literal_reference_is_valid(model: &CpModelProto, reference: i32) -> bool {
    if !variable_reference_is_valid(model, reference) {
        return false;
    }
    let var_proto = &model.variables[positive_ref(reference) as usize];
    let min_domain = var_proto.domain[0];
    let max_domain = var_proto.domain[var_proto.domain.len() - 1];
    min_domain >= 0 && max_domain <= 1
}

fn validate_integer_variable(model: &CpModelProto, v: usize) -> String {
    let proto: &IntegerVariableProto = &model.variables[v];
    if proto.domain.is_empty() {
        return format!(
            "var #{v} has no domain(): {}",
            protobuf_short_debug_string(proto)
        );
    }
    if !proto.domain.len().is_multiple_of(2) {
        return format!(
            "var #{v} has an odd domain() size: {}",
            protobuf_short_debug_string(proto)
        );
    }
    if !domain_in_proto_is_valid(proto) {
        return format!(
            "var #{v} has an invalid domain() format: {}",
            protobuf_short_debug_string(proto)
        );
    }

    // Internally, we often take the negation of a domain, and we also want to
    // have sentinel values greater than the min/max of a variable domain, so
    // the domain must fall in [-kint64max / 2, kint64max / 2].
    let lb = proto.domain[0];
    let ub = proto.domain[proto.domain.len() - 1];
    if lb < -i64::MAX / 2 || ub > i64::MAX / 2 {
        return format!(
            "var #{v} domain does not fall in [-kint64max / 2, kint64max / 2]. {}",
            protobuf_short_debug_string(proto)
        );
    }

    // We do compute ub - lb in some places in the code and do not want to deal
    // with overflow everywhere. This seems like a reasonable precondition anyway.
    if lb < 0 && lb + i64::MAX < ub {
        return format!(
            "var #{v} has a domain that is too large, i.e. |UB - LB| \
             overflows an int64_t: {}",
            protobuf_short_debug_string(proto)
        );
    }

    String::new()
}

fn validate_variables_used_in_constraint(model: &CpModelProto, c: usize) -> String {
    let ct = &model.constraints[c];
    let references = get_references_used_by_constraint(ct);
    for &v in &references.variables {
        if !variable_reference_is_valid(model, v) {
            return format!(
                "Out of bound integer variable {v} in constraint #{c} : {}",
                protobuf_short_debug_string(ct)
            );
        }
    }
    for &lit in &ct.enforcement_literal {
        if !literal_reference_is_valid(model, lit) {
            return format!(
                "Invalid enforcement literal {lit} in constraint #{c} : {}",
                protobuf_short_debug_string(ct)
            );
        }
    }
    for &lit in &references.literals {
        if !literal_reference_is_valid(model, lit) {
            return format!(
                "Invalid literal {lit} in constraint #{c} : {}",
                protobuf_short_debug_string(ct)
            );
        }
    }
    String::new()
}

fn validate_intervals_used_in_constraint(
    after_presolve: bool,
    model: &CpModelProto,
    c: usize,
) -> String {
    let ct = &model.constraints[c];
    for i in used_intervals(ct) {
        if i < 0 || i >= model.constraints.len() as i32 {
            return format!(
                "Out of bound interval {i} in constraint #{c} : {}",
                protobuf_short_debug_string(ct)
            );
        }
        if after_presolve && i >= c as i32 {
            return format!(
                "Interval {i} in constraint #{c} must appear before in the list of constraints :{}",
                protobuf_short_debug_string(ct)
            );
        }
        if constraint_case(&model.constraints[i as usize]) != ConstraintCase::Interval {
            return format!(
                "Interval {i} does not refer to an interval constraint. Problematic constraint #{c} : {}",
                protobuf_short_debug_string(ct)
            );
        }
    }
    String::new()
}

fn min_of_ref(model: &CpModelProto, r#ref: i32) -> i64 {
    let var_proto = &model.variables[positive_ref(r#ref) as usize];
    if ref_is_positive(r#ref) {
        var_proto.domain[0]
    } else {
        -var_proto.domain[var_proto.domain.len() - 1]
    }
}

fn max_of_ref(model: &CpModelProto, r#ref: i32) -> i64 {
    let var_proto = &model.variables[positive_ref(r#ref) as usize];
    if ref_is_positive(r#ref) {
        var_proto.domain[var_proto.domain.len() - 1]
    } else {
        -var_proto.domain[0]
    }
}

fn min_of_expression(model: &CpModelProto, proto: &LinearExpressionProto) -> i64 {
    let mut sum_min = proto.offset;
    for i in 0..proto.vars.len() {
        let r#ref = proto.vars[i];
        let coeff = proto.coeffs[i];
        sum_min = cap_add(
            sum_min,
            if coeff >= 0 {
                cap_prod(min_of_ref(model, r#ref), coeff)
            } else {
                cap_prod(max_of_ref(model, r#ref), coeff)
            },
        );
    }

    sum_min
}

fn max_of_expression(model: &CpModelProto, proto: &LinearExpressionProto) -> i64 {
    let mut sum_max = proto.offset;
    for i in 0..proto.vars.len() {
        let r#ref = proto.vars[i];
        let coeff = proto.coeffs[i];
        sum_max = cap_add(
            sum_max,
            if coeff >= 0 {
                cap_prod(max_of_ref(model, r#ref), coeff)
            } else {
                cap_prod(min_of_ref(model, r#ref), coeff)
            },
        );
    }

    sum_max
}

// The C++ getter `interval()` of a constraint of another kind returns the empty default message.
fn interval_of(ct: &ConstraintProto) -> &IntervalConstraintProto {
    static EMPTY: LazyLock<IntervalConstraintProto> =
        LazyLock::new(IntervalConstraintProto::default);
    match &ct.constraint {
        Some(Constraint::Interval(interval)) => interval,
        _ => &EMPTY,
    }
}

fn interval_size_max(model: &CpModelProto, interval_index: i32) -> i64 {
    debug_assert_eq!(
        ConstraintCase::Interval,
        constraint_case(&model.constraints[interval_index as usize])
    );
    let proto = interval_of(&model.constraints[interval_index as usize]);
    max_of_expression(model, or_empty(&proto.size))
}

fn domain_of_ref(model: &CpModelProto, r#ref: i32) -> Domain {
    let domain = read_domain_from_proto(&model.variables[positive_ref(r#ref) as usize]);
    if ref_is_positive(r#ref) {
        domain
    } else {
        domain.negation()
    }
}

fn validate_linear_expression(model: &CpModelProto, expr: &LinearExpressionProto) -> String {
    if expr.coeffs.len() != expr.vars.len() {
        return format!(
            "coeffs_size() != vars_size() in linear expression: {}",
            protobuf_short_debug_string(expr)
        );
    }
    for &var in &expr.vars {
        if !ref_is_positive(var) {
            return format!(
                "Invalid negated variable in linear expression: {}",
                protobuf_short_debug_string(expr)
            );
        }
    }
    if possible_integer_overflow(model, &expr.vars, &expr.coeffs, expr.offset, None) {
        return format!(
            "Possible overflow in linear expression: {}",
            protobuf_short_debug_string(expr)
        );
    }
    String::new()
}

fn validate_linear_constraint(
    model: &CpModelProto,
    ct: &ConstraintProto,
    after_presolve: bool,
) -> String {
    let Some(Constraint::Linear(arg)) = &ct.constraint else {
        unreachable!("a linear constraint is validated as such");
    };
    if !domain_in_proto_is_valid(arg) {
        return format!(
            "Invalid domain in constraint : {}",
            protobuf_short_debug_string(ct)
        );
    }
    if arg.coeffs.len() != arg.vars.len() {
        return format!(
            "coeffs_size() != vars_size() in constraint: {}",
            protobuf_short_debug_string(ct)
        );
    }
    for &var in &arg.vars {
        if !ref_is_positive(var) {
            return format!(
                "Invalid negated variable in linear constraint: {}",
                protobuf_short_debug_string(ct)
            );
        }
    }
    if possible_integer_overflow(model, &arg.vars, &arg.coeffs, 0, None) {
        return format!(
            "Possible integer overflow in constraint: {}",
            debug_string(ct)
        );
    }

    if after_presolve {
        // To avoid the need to check for overflow each time we do things like
        // (max|min_rhs) (+|-) (max|min_activity), we assume that the constraints
        // bounds can always be capped by the same bounds as the min/max activity of
        // a constraint.
        //
        // Note(fdid): The design decision of using tight bound simplify overflow
        // checking, but on another hand, to detect that a constraint is only one
        // sided (>= or <=) it require recomputing the activity rather than relying
        // on the bound to be canonicalized to +/- kint64max. So no clear winner
        // here, but we prefered not assuming canonicalization and just recomputing
        // the activity each time we need to decide if a constraint is only
        // one-sided.
        let min_rhs = arg.domain[0];
        let max_rhs = arg.domain[arg.domain.len() - 1];
        if min_rhs < -i64::MAX / 2 || max_rhs > i64::MAX / 2 {
            return format!(
                "Linear constraint domain was not tightenned: {}",
                debug_string(ct)
            );
        }
    }
    String::new()
}

fn validate_int_mod_constraint(model: &CpModelProto, ct: &ConstraintProto) -> String {
    let Some(Constraint::IntMod(arg)) = &ct.constraint else {
        unreachable!("an int_mod constraint is validated as such");
    };
    if arg.exprs.len() != 2 {
        return format!(
            "An int_mod constraint should have exactly 2 terms: {}",
            protobuf_short_debug_string(ct)
        );
    }
    if arg.target.is_none() {
        return format!(
            "An int_mod constraint should have a target: {}",
            protobuf_short_debug_string(ct)
        );
    }

    return_if_not_empty!(validate_linear_expression(model, &arg.exprs[0]));
    return_if_not_empty!(validate_linear_expression(model, &arg.exprs[1]));
    return_if_not_empty!(validate_linear_expression(model, or_empty(&arg.target)));

    let mod_expr = &arg.exprs[1];
    if min_of_expression(model, mod_expr) <= 0 {
        return format!(
            "An int_mod must have a strictly positive modulo argument: {}",
            protobuf_short_debug_string(ct)
        );
    }

    String::new()
}

fn validate_int_prod_constraint(model: &CpModelProto, ct: &ConstraintProto) -> String {
    let Some(Constraint::IntProd(arg)) = &ct.constraint else {
        unreachable!("an int_prod constraint is validated as such");
    };
    if arg.target.is_none() {
        return format!(
            "An int_prod constraint should have a target: {}",
            protobuf_short_debug_string(ct)
        );
    }

    for expr in &arg.exprs {
        return_if_not_empty!(validate_linear_expression(model, expr));
    }
    return_if_not_empty!(validate_linear_expression(model, or_empty(&arg.target)));

    // Detect potential overflow.
    let mut product_domain = Domain::new_value(1);
    for expr in &arg.exprs {
        let min_expr = min_of_expression(model, expr);
        let max_expr = max_of_expression(model, expr);
        if min_expr == 0 && max_expr == 0 {
            // An overflow multiplied by zero is still invalid.
            continue;
        }
        product_domain =
            product_domain.continuous_multiplication_by_domain(&Domain::new(min_expr, max_expr));
    }

    if product_domain.max() <= -i64::MAX || product_domain.min() == i64::MAX {
        return format!(
            "integer overflow in constraint: {}",
            protobuf_short_debug_string(ct)
        );
    }

    // We need to expand the product when its arity is > 2. In that case, we must
    // be strict with overflows.
    if arg.exprs.len() > 2
        && (product_domain.max() == i64::MAX || product_domain.min() <= -i64::MAX)
    {
        return format!(
            "Potential integer overflow in constraint: {}",
            protobuf_short_debug_string(ct)
        );
    }

    String::new()
}

fn validate_int_div_constraint(model: &CpModelProto, ct: &ConstraintProto) -> String {
    let Some(Constraint::IntDiv(arg)) = &ct.constraint else {
        unreachable!("an int_div constraint is validated as such");
    };
    if arg.exprs.len() != 2 {
        return format!(
            "An int_div constraint should have exactly 2 terms: {}",
            protobuf_short_debug_string(ct)
        );
    }
    if arg.target.is_none() {
        return format!(
            "An int_div constraint should have a target: {}",
            protobuf_short_debug_string(ct)
        );
    }

    return_if_not_empty!(validate_linear_expression(model, &arg.exprs[0]));
    return_if_not_empty!(validate_linear_expression(model, &arg.exprs[1]));
    return_if_not_empty!(validate_linear_expression(model, or_empty(&arg.target)));

    // Canonicalize the divisor: merge repeated variables and fold fixed
    // variables into the offset, so that the checks below see the expression
    // the solver will actually divide by.
    //
    // TODO(user): Remove all this complexity by simply handling any kind of
    // divisor. Just document/enforce that any feasible solution of an enforced
    // int_div must have a divisor that is non-zero.
    let denom = &arg.exprs[1];
    let mut offset = denom.offset;
    let mut terms: BTreeMap<i32, i64> = BTreeMap::new();
    for i in 0..denom.vars.len() {
        let var = denom.vars[i];
        let coeff = denom.coeffs[i];
        if coeff == 0 {
            continue;
        }
        if min_of_ref(model, var) == max_of_ref(model, var) {
            offset += coeff * min_of_ref(model, var);
        } else {
            *terms.entry(var).or_insert(0) += coeff;
        }
    }
    terms.retain(|_, coeff| *coeff != 0);

    let divisor_can_be_zero: bool = if terms.is_empty() {
        if offset == 0 {
            return format!("Division by 0: {}", protobuf_short_debug_string(ct));
        }
        false
    } else if terms.len() == 1 {
        // Exact check for an affine divisor, so that a domain with a hole at zero
        // is accepted.
        let (&var, &coeff) = terms.iter().next().expect("the map has one term");
        let inverse_of_zero = -offset / coeff;
        inverse_of_zero * coeff + offset == 0 && domain_of_ref(model, var).contains(inverse_of_zero)
    } else if terms.len() == 2 {
        // Also exact with two terms.
        let mut term_iter = terms.iter();
        let (&var_a, &coeff_a) = term_iter.next().expect("the map has two terms");
        let (&var_b, &coeff_b) = term_iter.next().expect("the map has two terms");
        diophantine_equation_of_size_two_has_solution_in_domain(
            &domain_of_ref(model, var_a),
            coeff_a,
            &domain_of_ref(model, var_b),
            coeff_b,
            -offset,
        )
    } else {
        // With more terms, the divisor can only be 0 if the gcd of the
        // coefficients divides the offset and 0 is within its bounds.
        let mut gcd: i64 = 0;
        let mut min = offset;
        let mut max = offset;
        for (&var, &coeff) in &terms {
            gcd = std_gcd(gcd, coeff.abs());
            min += if coeff > 0 {
                coeff * min_of_ref(model, var)
            } else {
                coeff * max_of_ref(model, var)
            };
            max += if coeff > 0 {
                coeff * max_of_ref(model, var)
            } else {
                coeff * min_of_ref(model, var)
            };
        }
        offset % gcd == 0 && min <= 0 && max >= 0
    };
    if divisor_can_be_zero {
        return format!(
            "The domain of the divisor cannot contain 0: {}",
            protobuf_short_debug_string(ct)
        );
    }
    String::new()
}

fn append_to_overflow_validator(
    input: &LinearExpressionProto,
    output: &mut LinearExpressionProto,
    prod: i64,
) {
    output.vars.extend_from_slice(&input.vars);
    for &coeff in &input.coeffs {
        output.coeffs.push(coeff * prod);
    }

    // We add the absolute value to be sure that future computations will not
    // overflow depending on the order they are performed in.
    output.offset = cap_add(output.offset.abs(), input.offset.abs());
}

fn validate_element_constraint(model: &CpModelProto, ct: &ConstraintProto) -> String {
    let Some(Constraint::Element(element)) = &ct.constraint else {
        unreachable!("an element constraint is validated as such");
    };

    let in_linear_format = element.linear_index.is_some()
        || element.linear_target.is_some()
        || !element.exprs.is_empty();
    let in_legacy_format = !element.vars.is_empty() || element.index != 0 || element.target != 0;
    if in_linear_format && in_legacy_format {
        return format!(
            "Inconsistent element with both legacy and new format defined{}",
            protobuf_short_debug_string(ct)
        );
    }

    if element.vars.is_empty() && element.exprs.is_empty() {
        return "Empty element constraint is interpreted as vars[], thus invalid \
                since the index will be out of bounds."
            .to_owned();
    }

    if in_legacy_format
        && (!variable_index_is_valid(model, element.index)
            || !variable_index_is_valid(model, element.target))
    {
        return format!(
            "Element constraint index and target must be valid variables: {}",
            protobuf_short_debug_string(ct)
        );
    }

    // We need to be able to manipulate expressions like "target - var" without
    // integer overflow.
    if !element.vars.is_empty() {
        let mut overflow_detection = LinearExpressionProto::default();
        overflow_detection.vars.push(element.target);
        overflow_detection.coeffs.push(1);
        overflow_detection.vars.push(/*dummy*/ 0);
        overflow_detection.coeffs.push(-1);
        for &r#ref in &element.vars {
            if !variable_index_is_valid(model, r#ref) {
                return format!(
                    "Element vars must be valid variables: {}",
                    protobuf_short_debug_string(ct)
                );
            }
            overflow_detection.vars[1] = r#ref;
            if possible_integer_overflow(
                model,
                &overflow_detection.vars,
                &overflow_detection.coeffs,
                0,
                None,
            ) {
                return format!(
                    "Domain of the variables involved in element constraint may cause \
                     overflow{}",
                    protobuf_short_debug_string(ct)
                );
            }
        }
    }

    if in_linear_format {
        return_if_not_empty!(validate_linear_expression(
            model,
            or_empty(&element.linear_index)
        ));
        return_if_not_empty!(validate_linear_expression(
            model,
            or_empty(&element.linear_target)
        ));
        for expr in &element.exprs {
            return_if_not_empty!(validate_linear_expression(model, expr));
            let mut overflow_detection = or_empty(&element.linear_target).clone();
            append_to_overflow_validator(expr, &mut overflow_detection, -1);
            let offset = cap_sub(overflow_detection.offset, expr.offset);
            overflow_detection.offset = offset;
            if possible_integer_overflow(
                model,
                &overflow_detection.vars,
                &overflow_detection.coeffs,
                overflow_detection.offset,
                None,
            ) {
                return "Domain of the variables involved in element constraint may cause \
                        overflow"
                    .to_owned();
            }
        }
    }

    String::new()
}

fn validate_inverse_constraint(model: &CpModelProto, ct: &ConstraintProto) -> String {
    let Some(Constraint::Inverse(inverse)) = &ct.constraint else {
        unreachable!("an inverse constraint is validated as such");
    };
    if inverse.f_direct.len() != inverse.f_inverse.len()
        || inverse.f_expr_direct.len() != inverse.f_expr_inverse.len()
    {
        return format!(
            "Non-matching fields size in inverse: {}",
            protobuf_short_debug_string(ct)
        );
    }
    if !inverse.f_inverse.is_empty() && !inverse.f_expr_inverse.is_empty() {
        return format!(
            "Inconsistent inverse with both legacy and new format defined: {}",
            protobuf_short_debug_string(ct)
        );
    }
    if !inverse.f_direct.is_empty() {
        for vars in [&inverse.f_direct, &inverse.f_inverse] {
            for &var in vars {
                if !variable_index_is_valid(model, var) {
                    return format!("Invalid variable index in inverse constraint: {var}");
                }
            }
        }
    }
    if !inverse.f_expr_direct.is_empty() {
        for expr in &inverse.f_expr_direct {
            return_if_not_empty!(validate_linear_expression(model, expr));
        }
        for expr in &inverse.f_expr_inverse {
            return_if_not_empty!(validate_linear_expression(model, expr));
        }
    }
    String::new()
}

fn validate_table_constraint(model: &CpModelProto, ct: &ConstraintProto) -> String {
    let Some(Constraint::Table(arg)) = &ct.constraint else {
        unreachable!("a table constraint is validated as such");
    };
    if !arg.vars.is_empty() && !arg.exprs.is_empty() {
        return format!(
            "Inconsistent table with both legacy and new format defined: {}",
            protobuf_short_debug_string(ct)
        );
    }
    if arg.vars.is_empty() && arg.exprs.is_empty() && !arg.values.is_empty() {
        return format!(
            "Inconsistent table with empty expressions and non-empty tuples: {}",
            protobuf_short_debug_string(ct)
        );
    }
    if arg.vars.is_empty() && arg.exprs.is_empty() && arg.values.is_empty() {
        return String::new();
    }
    let arity = if arg.vars.is_empty() {
        arg.exprs.len()
    } else {
        arg.vars.len()
    };
    if arg.values.len() % arity != 0 {
        return format!(
            "The flat encoding of table constraint tuples must be a multiple of \
             the number of expressions: {}",
            debug_string(ct)
        );
    }
    for &var in &arg.vars {
        if !variable_index_is_valid(model, var) {
            return format!("Invalid variable index in table constraint: {var}");
        }
    }
    for expr in &arg.exprs {
        return_if_not_empty!(validate_linear_expression(model, expr));
    }
    String::new()
}

fn validate_automaton_constraint(model: &CpModelProto, ct: &ConstraintProto) -> String {
    let Some(Constraint::Automaton(automaton)) = &ct.constraint else {
        unreachable!("an automaton constraint is validated as such");
    };
    if !automaton.vars.is_empty() && !automaton.exprs.is_empty() {
        return format!(
            "Inconsistent automaton with both legacy and new format defined: {}",
            protobuf_short_debug_string(ct)
        );
    }
    let num_transitions = automaton.transition_tail.len();
    if num_transitions != automaton.transition_head.len()
        || num_transitions != automaton.transition_label.len()
    {
        return format!(
            "The transitions repeated fields must have the same size: {}",
            protobuf_short_debug_string(ct)
        );
    }
    for &var in &automaton.vars {
        if !variable_index_is_valid(model, var) {
            return format!("Invalid variable index in automaton constraint: {var}");
        }
    }
    for expr in &automaton.exprs {
        return_if_not_empty!(validate_linear_expression(model, expr));
    }
    let mut tail_label_to_head: HashMap<(i64, i64), i64> = HashMap::new();
    for i in 0..num_transitions {
        let tail = automaton.transition_tail[i];
        let head = automaton.transition_head[i];
        let label = automaton.transition_label[i];
        if label <= i64::MIN + 1 || label == i64::MAX {
            return format!("labels in the automaton constraint are too big: {label}");
        }
        if let Some(&existing_head) = tail_label_to_head.get(&(tail, label)) {
            if existing_head == head {
                return format!("automaton: duplicate transition {tail} --({label})--> {head}");
            } else {
                return format!(
                    "automaton: incompatible transitions {tail} --({label})--> {head} and {tail} --({label})--> {existing_head}"
                );
            }
        }
        tail_label_to_head.insert((tail, label), head);
    }
    String::new()
}

// The fields that the circuit and the routes constraints share.
trait GraphProto: prost::Message + prost::Name + Default {
    fn tails(&self) -> &[i32];
    fn heads(&self) -> &[i32];
    fn literals(&self) -> &[i32];
}

macro_rules! impl_graph_proto {
    ($($proto:ty),+) => {
        $(
            impl GraphProto for $proto {
                fn tails(&self) -> &[i32] {
                    &self.tails
                }

                fn heads(&self) -> &[i32] {
                    &self.heads
                }

                fn literals(&self) -> &[i32] {
                    &self.literals
                }
            }
        )+
    };
}

impl_graph_proto!(CircuitConstraintProto, RoutesConstraintProto);

fn validate_graph_input<G: GraphProto>(is_route: bool, graph: &G) -> String {
    let size = graph.tails().len();
    if graph.heads().len() != size || graph.literals().len() != size {
        return format!(
            "Wrong field sizes in graph: {}",
            protobuf_short_debug_string(graph)
        );
    }

    // We currently disallow multiple self-loops on the same node.
    let mut self_loops: HashSet<i32> = HashSet::new();
    for i in 0..size {
        if graph.heads()[i] != graph.tails()[i] {
            continue;
        }
        if !self_loops.insert(graph.heads()[i]) {
            return format!(
                "Circuit/Route constraint contains multiple self-loops involving \
                 node {}",
                graph.heads()[i]
            );
        }
        if is_route && graph.tails()[i] == 0 {
            return "A route constraint cannot have a self-loop on the depot (node 0)".to_owned();
        }
    }

    String::new()
}

fn validate_routes_constraint(model: &CpModelProto, ct: &ConstraintProto) -> String {
    let Some(Constraint::Routes(routes)) = &ct.constraint else {
        unreachable!("a routes constraint is validated as such");
    };
    let mut max_node: i32 = 0;
    let mut nodes: HashSet<i32> = HashSet::new();
    for &node in &routes.tails {
        if node < 0 {
            return "All nodes in a route constraint must be in [0, num_nodes)".to_owned();
        }
        nodes.insert(node);
        max_node = max_node.max(node);
    }
    for &node in &routes.heads {
        if node < 0 {
            return "All nodes in a route constraint must be in [0, num_nodes)".to_owned();
        }
        nodes.insert(node);
        max_node = max_node.max(node);
    }
    if !nodes.is_empty() && max_node as usize != nodes.len() - 1 {
        return "All nodes in a route constraint must have incident arcs".to_owned();
    }

    for dimension in &routes.dimensions {
        if dimension.exprs.len() != nodes.len() {
            return format!(
                "If the dimensions field in a route constraint is set, its elements \
                 must be of size num_nodes:{}",
                nodes.len()
            );
        }
        for expr in &dimension.exprs {
            for &v in &expr.vars {
                if !variable_reference_is_valid(model, v) {
                    return format!(
                        "Out of bound integer variable {v} in route constraint {}",
                        protobuf_short_debug_string(ct)
                    );
                }
            }
            return_if_not_empty!(validate_linear_expression(model, expr));
        }
    }

    validate_graph_input(true, routes)
}

fn validate_interval_constraint(model: &CpModelProto, ct: &ConstraintProto) -> String {
    let Some(Constraint::Interval(arg)) = &ct.constraint else {
        unreachable!("an interval constraint is validated as such");
    };

    if arg.start.is_none() {
        return format!(
            "Interval must have a start expression: {}",
            protobuf_short_debug_string(ct)
        );
    }
    if arg.size.is_none() {
        return format!(
            "Interval must have a size expression: {}",
            protobuf_short_debug_string(ct)
        );
    }
    if arg.end.is_none() {
        return format!(
            "Interval must have an end expression: {}",
            protobuf_short_debug_string(ct)
        );
    }

    let mut for_overflow_validation = LinearExpressionProto::default();
    if or_empty(&arg.start).vars.len() > 1 {
        return "Intervals with a start expression containing more than one \
                variable are currently not supported."
            .to_owned();
    }
    return_if_not_empty!(validate_linear_expression(model, or_empty(&arg.start)));
    append_to_overflow_validator(or_empty(&arg.start), &mut for_overflow_validation, 1);
    if or_empty(&arg.size).vars.len() > 1 {
        return "Intervals with a size expression containing more than one \
                variable are currently not supported."
            .to_owned();
    }
    return_if_not_empty!(validate_linear_expression(model, or_empty(&arg.size)));
    if ct.enforcement_literal.is_empty() && min_of_expression(model, or_empty(&arg.size)) < 0 {
        return format!(
            "The size of a performed interval must be >= 0 in constraint: {}",
            debug_string(ct)
        );
    }
    append_to_overflow_validator(or_empty(&arg.size), &mut for_overflow_validation, 1);
    if or_empty(&arg.end).vars.len() > 1 {
        return "Intervals with an end expression containing more than one \
                variable are currently not supported."
            .to_owned();
    }
    return_if_not_empty!(validate_linear_expression(model, or_empty(&arg.end)));
    append_to_overflow_validator(or_empty(&arg.end), &mut for_overflow_validation, -1);

    if possible_integer_overflow(
        model,
        &for_overflow_validation.vars,
        &for_overflow_validation.coeffs,
        for_overflow_validation.offset,
        None,
    ) {
        return format!(
            "Possible overflow in interval: {}",
            protobuf_short_debug_string(arg)
        );
    }

    String::new()
}

fn validate_cumulative_constraint(model: &CpModelProto, ct: &ConstraintProto) -> String {
    let Some(Constraint::Cumulative(cumulative)) = &ct.constraint else {
        unreachable!("a cumulative constraint is validated as such");
    };
    if cumulative.intervals.len() != cumulative.demands.len() {
        return format!(
            "intervals_size() != demands_size() in constraint: {}",
            protobuf_short_debug_string(ct)
        );
    }

    return_if_not_empty!(validate_linear_expression(
        model,
        or_empty(&cumulative.capacity)
    ));
    for demand in &cumulative.demands {
        return_if_not_empty!(validate_linear_expression(model, demand));
    }

    for demand_expr in &cumulative.demands {
        if min_of_expression(model, demand_expr) < 0 {
            return format!(
                "Demand {} must be positive in constraint: {}",
                debug_string(demand_expr),
                debug_string(ct)
            );
        }
        if demand_expr.vars.len() > 1 {
            return format!(
                "Demand {} must be affine or constant in constraint: {}",
                debug_string(demand_expr),
                debug_string(ct)
            );
        }
    }
    if or_empty(&cumulative.capacity).vars.len() > 1 {
        return format!(
            "capacity {} must be affine or constant in constraint: {}",
            debug_string(or_empty(&cumulative.capacity)),
            debug_string(ct)
        );
    }

    let mut sum_max_demands: i64 = 0;
    for demand_expr in &cumulative.demands {
        let demand_max = max_of_expression(model, demand_expr);
        debug_assert!(demand_max >= 0);
        sum_max_demands = cap_add(sum_max_demands, demand_max);
        if sum_max_demands == i64::MAX {
            return format!(
                "The sum of max demands does not fit on an int64_t in constraint: {}",
                debug_string(ct)
            );
        }
    }

    String::new()
}

fn validate_no_overlap_2d_constraint(model: &CpModelProto, ct: &ConstraintProto) -> String {
    let Some(Constraint::NoOverlap2d(no_overlap_2d)) = &ct.constraint else {
        unreachable!("a no_overlap_2d constraint is validated as such");
    };
    let size_x = no_overlap_2d.x_intervals.len();
    let size_y = no_overlap_2d.y_intervals.len();
    if size_x != size_y {
        return format!(
            "The two lists of intervals must have the same size: {}",
            protobuf_short_debug_string(ct)
        );
    }

    // Checks if the sum of max areas of each rectangle can overflow.
    let mut sum_max_areas: i64 = 0;
    for i in 0..no_overlap_2d.x_intervals.len() {
        let max_size_x = interval_size_max(model, no_overlap_2d.x_intervals[i]);
        let max_size_y = interval_size_max(model, no_overlap_2d.y_intervals[i]);
        sum_max_areas = cap_add(sum_max_areas, cap_prod(max_size_x, max_size_y));
        if sum_max_areas == i64::MAX {
            return format!(
                "Integer overflow when summing all areas in constraint: {}",
                debug_string(ct)
            );
        }
    }
    String::new()
}

fn validate_reservoir_constraint(model: &CpModelProto, ct: &ConstraintProto) -> String {
    let Some(Constraint::Reservoir(reservoir)) = &ct.constraint else {
        unreachable!("a reservoir constraint is validated as such");
    };
    if reservoir.time_exprs.len() != reservoir.level_changes.len() {
        return format!(
            "time_exprs and level_changes fields must be of the same size: {}",
            protobuf_short_debug_string(ct)
        );
    }
    for expr in &reservoir.time_exprs {
        return_if_not_empty!(validate_linear_expression(model, expr));
        // We want to be able to safely put time_exprs[i]-time_exprs[j] in a linear.
        if min_of_expression(model, expr) <= -i64::MAX / 4
            || max_of_expression(model, expr) >= i64::MAX / 4
        {
            return format!(
                "Potential integer overflow on time_expr of a reservoir: {}",
                protobuf_short_debug_string(ct)
            );
        }
    }
    for expr in &reservoir.level_changes {
        return_if_not_empty!(validate_linear_expression(model, expr));
    }
    if reservoir.min_level > 0 {
        return format!(
            "The min level of a reservoir must be <= 0. Please use fixed events to \
             set up initial state: {}",
            protobuf_short_debug_string(ct)
        );
    }
    if reservoir.max_level < 0 {
        return format!(
            "The max level of a reservoir must be >= 0. Please use fixed events to \
             set up initial state: {}",
            protobuf_short_debug_string(ct)
        );
    }

    let mut sum_abs: i64 = 0;
    for demand in &reservoir.level_changes {
        // We test for min int64_t before the abs().
        let demand_min = min_of_expression(model, demand);
        let demand_max = max_of_expression(model, demand);
        sum_abs = cap_add(sum_abs, cap_abs(demand_min).max(cap_abs(demand_max)));
        if sum_abs == i64::MAX {
            return format!(
                "Possible integer overflow in constraint: {}",
                debug_string(ct)
            );
        }
    }
    if !reservoir.active_literals.is_empty()
        && reservoir.active_literals.len() != reservoir.time_exprs.len()
    {
        return "Wrong array length of active_literals variables".to_owned();
    }
    if !reservoir.level_changes.is_empty()
        && reservoir.level_changes.len() != reservoir.time_exprs.len()
    {
        return "Wrong array length of level_changes variables".to_owned();
    }
    String::new()
}

fn validate_objective(model: &CpModelProto, obj: &CpObjectiveProto) -> String {
    if !domain_in_proto_is_valid(obj) {
        return format!(
            "The objective has an invalid domain() format: {}",
            protobuf_short_debug_string(obj)
        );
    }
    if obj.vars.len() != obj.coeffs.len() {
        return format!(
            "vars and coeffs sizes do not match in objective: {}",
            protobuf_short_debug_string(obj)
        );
    }
    for &v in &obj.vars {
        if !variable_index_is_valid(model, v) {
            return format!(
                "Out of bound integer variable {v} in objective: {}",
                protobuf_short_debug_string(obj)
            );
        }
    }
    let mut bounds: (i64, i64) = (0, 0);
    if possible_integer_overflow(model, &obj.vars, &obj.coeffs, 0, Some(&mut bounds)) {
        return format!(
            "Possible integer overflow in objective: {}",
            debug_string(obj)
        );
    }
    if !obj.offset.is_finite() {
        return format!("Objective offset must be finite: {}", debug_string(obj));
    }
    if obj.scaling_factor != 0.0
        && obj.scaling_factor != 1.0
        && obj.scaling_factor != -1.0
        && (!((obj.scaling_factor * bounds.0 as f64).abs() + obj.offset.abs()).is_finite()
            || !((obj.scaling_factor * bounds.1 as f64).abs() + obj.offset.abs()).is_finite())
    {
        return format!(
            "Possible floating point overflow in objective when multiplied by \
             the scaling factor: {}",
            debug_string(obj)
        );
    }
    String::new()
}

fn validate_floating_point_objective(
    max_valid_magnitude: f64,
    model: &CpModelProto,
    obj: &FloatObjectiveProto,
) -> String {
    if obj.vars.len() != obj.coeffs.len() {
        return format!(
            "vars and coeffs sizes do not match in objective: {}",
            protobuf_short_debug_string(obj)
        );
    }
    for &v in &obj.vars {
        if !variable_index_is_valid(model, v) {
            return format!(
                "Out of bound integer variable {v} in objective: {}",
                protobuf_short_debug_string(obj)
            );
        }
    }
    for &coeff in &obj.coeffs {
        if !coeff.is_finite() {
            return format!(
                "Coefficients must be finite in objective: {}",
                protobuf_short_debug_string(obj)
            );
        }
        if coeff.abs() > max_valid_magnitude {
            return format!(
                "Coefficients larger than params.mip_max_valid_magnitude() [value = {}] in objective: {}",
                six_digits_to_string(max_valid_magnitude),
                protobuf_short_debug_string(obj)
            );
        }
    }
    if !obj.offset.is_finite() {
        return format!(
            "Offset must be finite in objective: {}",
            protobuf_short_debug_string(obj)
        );
    }
    let mut sum_min: f64 = obj.offset;
    let mut sum_max: f64 = obj.offset;
    for i in 0..obj.vars.len() {
        let r#ref = obj.vars[i];
        let var_proto = &model.variables[positive_ref(r#ref) as usize];
        let min_domain = var_proto.domain[0];
        let max_domain = var_proto.domain[var_proto.domain.len() - 1];
        let coeff: f64 = if ref_is_positive(r#ref) {
            obj.coeffs[i]
        } else {
            -obj.coeffs[i]
        };
        let prod1: f64 = min_domain as f64 * coeff;
        let prod2: f64 = max_domain as f64 * coeff;

        // Note that we use min/max with zero to disallow "alternative" terms and
        // be sure that we cannot have an overflow if we do the computation in a
        // different order.
        sum_min += 0.0_f64.min(prod1.min(prod2));
        sum_max += 0.0_f64.max(prod1.max(prod2));
    }
    if !(2.0 * sum_min).is_finite() || !(2.0 * sum_max).is_finite() {
        return format!(
            "Possible floating point overflow in objective: {}",
            protobuf_short_debug_string(obj)
        );
    }
    String::new()
}

fn validate_search_strategies(model: &CpModelProto) -> String {
    for strategy in &model.search_strategy {
        let vss = strategy.variable_selection_strategy;
        if vss != VariableSelectionStrategy::ChooseFirst as i32
            && vss != VariableSelectionStrategy::ChooseLowestMin as i32
            && vss != VariableSelectionStrategy::ChooseHighestMax as i32
            && vss != VariableSelectionStrategy::ChooseMinDomainSize as i32
            && vss != VariableSelectionStrategy::ChooseMaxDomainSize as i32
        {
            return format!("Unknown or unsupported variable_selection_strategy: {vss}");
        }
        let drs = strategy.domain_reduction_strategy;
        if drs != DomainReductionStrategy::SelectMinValue as i32
            && drs != DomainReductionStrategy::SelectMaxValue as i32
            && drs != DomainReductionStrategy::SelectLowerHalf as i32
            && drs != DomainReductionStrategy::SelectUpperHalf as i32
            && drs != DomainReductionStrategy::SelectMedianValue as i32
            && drs != DomainReductionStrategy::SelectRandomHalf as i32
        {
            return format!("Unknown or unsupported domain_reduction_strategy: {drs}");
        }
        if !strategy.variables.is_empty() && !strategy.exprs.is_empty() {
            return format!(
                "Strategy can't have both variables and exprs: {}",
                protobuf_short_debug_string(strategy)
            );
        }
        for &r#ref in &strategy.variables {
            if !variable_reference_is_valid(model, r#ref) {
                return format!(
                    "Invalid variable reference in strategy: {}",
                    protobuf_short_debug_string(strategy)
                );
            }
            if drs == DomainReductionStrategy::SelectMedianValue as i32
                && read_domain_from_proto(&model.variables[positive_ref(r#ref) as usize]).size()
                    > 100000
            {
                return format!(
                    "Variable #{} has a domain too large to be used in a \
                     SELECT_MEDIAN_VALUE value selection strategy",
                    positive_ref(r#ref)
                );
            }
        }
        for expr in &strategy.exprs {
            if expr.vars.len() > 1 {
                return format!(
                    "expression must be affine in strategy: {}",
                    protobuf_short_debug_string(strategy)
                );
            }
            for &var in &expr.vars {
                if !variable_reference_is_valid(model, var) {
                    return format!(
                        "Invalid variable reference in strategy: {}",
                        protobuf_short_debug_string(strategy)
                    );
                }
            }
            if !validate_linear_expression(model, expr).is_empty() {
                return format!(
                    "Invalid affine expr in strategy: {}",
                    protobuf_short_debug_string(strategy)
                );
            }
            if drs == DomainReductionStrategy::SelectMedianValue as i32 {
                for &var in &expr.vars {
                    if read_domain_from_proto(&model.variables[var as usize]).size() > 100000 {
                        return format!(
                            "Variable #{var} has a domain too large to be used in a \
                             SELECT_MEDIAN_VALUE value selection strategy"
                        );
                    }
                }
            }
        }
    }
    String::new()
}

fn validate_solution_hint(model: &CpModelProto) -> String {
    let Some(hint) = &model.solution_hint else {
        return String::new();
    };
    if hint.vars.len() != hint.values.len() {
        return "Invalid solution hint: vars and values do not have the same size.".to_owned();
    }
    for &var in &hint.vars {
        if !variable_index_is_valid(model, var) {
            return format!("Invalid variable in solution hint: {var}");
        }
    }

    // Reject hints with duplicate variables as this is likely a user error.
    let mut indices: HashSet<i32> = HashSet::new();
    for &var in &hint.vars {
        if !indices.insert(positive_ref(var)) {
            return format!(
                "The solution hint contains duplicate variables like the variable \
                 with index #{}",
                positive_ref(var)
            );
        }
    }

    // Reject hints equal to INT_MIN or INT_MAX.
    for &value in &hint.values {
        if value == i64::MIN || value == i64::MAX {
            return "The solution hint cannot contain the INT_MIN or INT_MAX values.".to_owned();
        }
    }

    String::new()
}

// =============================================================================
// Solution Feasibility.
// =============================================================================

// The methods take the payload of the constraint, which `constraint_is_feasible`
// picks by kind, where upstream takes the constraint and reads the payload again.
struct ConstraintChecker {
    variable_values: Vec<i64>,
}

impl ConstraintChecker {
    fn new(variable_values: &[i64]) -> Self {
        ConstraintChecker {
            variable_values: variable_values.to_vec(),
        }
    }

    fn literal_is_true(&self, l: i32) -> bool {
        if l >= 0 {
            return self.variable_values[l as usize] != 0;
        }
        self.variable_values[(-l - 1) as usize] == 0
    }

    fn literal_is_false(&self, l: i32) -> bool {
        !self.literal_is_true(l)
    }

    fn value(&self, var: i32) -> i64 {
        if var >= 0 {
            return self.variable_values[var as usize];
        }
        -self.variable_values[(-var - 1) as usize]
    }

    fn constraint_is_enforced(&self, ct: &ConstraintProto) -> bool {
        for &lit in &ct.enforcement_literal {
            if self.literal_is_false(lit) {
                return false;
            }
        }
        true
    }

    fn bool_or_constraint_is_feasible(&self, arg: &BoolArgumentProto) -> bool {
        for &lit in &arg.literals {
            if self.literal_is_true(lit) {
                return true;
            }
        }
        false
    }

    fn bool_and_constraint_is_feasible(&self, arg: &BoolArgumentProto) -> bool {
        for &lit in &arg.literals {
            if self.literal_is_false(lit) {
                return false;
            }
        }
        true
    }

    fn at_most_one_constraint_is_feasible(&self, arg: &BoolArgumentProto) -> bool {
        let mut num_true_literals = 0;
        for &lit in &arg.literals {
            if self.literal_is_true(lit) {
                num_true_literals += 1;
            }
        }
        num_true_literals <= 1
    }

    fn exactly_one_constraint_is_feasible(&self, arg: &BoolArgumentProto) -> bool {
        let mut num_true_literals = 0;
        for &lit in &arg.literals {
            if self.literal_is_true(lit) {
                num_true_literals += 1;
            }
        }
        num_true_literals == 1
    }

    fn bool_xor_constraint_is_feasible(&self, arg: &BoolArgumentProto) -> bool {
        let mut sum = 0;
        for &lit in &arg.literals {
            sum ^= i32::from(self.literal_is_true(lit));
        }
        sum == 1
    }

    fn linear_constraint_is_feasible(&self, arg: &LinearConstraintProto) -> bool {
        let mut sum: i64 = 0;
        let num_variables = arg.coeffs.len();
        let vars = &arg.vars;
        let coeffs = &arg.coeffs;
        for i in 0..num_variables {
            // We know we only have positive references now.
            debug_assert!(ref_is_positive(vars[i]));
            sum += self.variable_values[vars[i] as usize] * coeffs[i];
        }
        let result = domain_in_proto_contains(arg, sum);
        if !result {
            log::debug!("Activity: {sum}");
        }
        result
    }

    fn linear_expression_value(&self, expr: &LinearExpressionProto) -> i64 {
        let mut sum: i64 = expr.offset;
        let num_variables = expr.vars.len();
        for i in 0..num_variables {
            sum += self.value(expr.vars[i]) * expr.coeffs[i];
        }
        sum
    }

    fn lin_max_constraint_is_feasible(&self, arg: &LinearArgumentProto) -> bool {
        let max = self.linear_expression_value(or_empty(&arg.target));
        let mut actual_max = i64::MIN;
        for i in 0..arg.exprs.len() {
            let expr_value = self.linear_expression_value(&arg.exprs[i]);
            actual_max = actual_max.max(expr_value);
        }
        max == actual_max
    }

    fn int_prod_constraint_is_feasible(&self, arg: &LinearArgumentProto) -> bool {
        let prod = self.linear_expression_value(or_empty(&arg.target));
        let mut actual_prod: i64 = 1;
        for expr in &arg.exprs {
            actual_prod = cap_prod(actual_prod, self.linear_expression_value(expr));
        }
        prod == actual_prod
    }

    fn int_div_constraint_is_feasible(&self, arg: &LinearArgumentProto) -> bool {
        self.linear_expression_value(or_empty(&arg.target))
            == self.linear_expression_value(&arg.exprs[0])
                / self.linear_expression_value(&arg.exprs[1])
    }

    fn int_mod_constraint_is_feasible(&self, arg: &LinearArgumentProto) -> bool {
        self.linear_expression_value(or_empty(&arg.target))
            == self.linear_expression_value(&arg.exprs[0])
                % self.linear_expression_value(&arg.exprs[1])
    }

    fn all_diff_constraint_is_feasible(&self, arg: &AllDifferentConstraintProto) -> bool {
        let mut values: HashSet<i64> = HashSet::new();
        for expr in &arg.exprs {
            let value = self.linear_expression_value(expr);
            if !values.insert(value) {
                return false;
            }
        }
        true
    }

    fn interval_start(&self, interval: &IntervalConstraintProto) -> i64 {
        self.linear_expression_value(or_empty(&interval.start))
    }

    fn interval_size(&self, interval: &IntervalConstraintProto) -> i64 {
        self.linear_expression_value(or_empty(&interval.size))
    }

    fn interval_end(&self, interval: &IntervalConstraintProto) -> i64 {
        self.linear_expression_value(or_empty(&interval.end))
    }

    fn interval_constraint_is_feasible(&self, interval: &IntervalConstraintProto) -> bool {
        let size = self.interval_size(interval);
        if size < 0 {
            return false;
        }
        self.interval_start(interval) + size == self.interval_end(interval)
    }

    fn no_overlap_constraint_is_feasible(
        &self,
        model: &CpModelProto,
        arg: &NoOverlapConstraintProto,
    ) -> bool {
        let mut start_durations_pairs: Vec<(i64, i64)> = Vec::new();
        for &i in &arg.intervals {
            let interval_constraint = &model.constraints[i as usize];
            if self.constraint_is_enforced(interval_constraint) {
                let interval = interval_of(interval_constraint);
                start_durations_pairs
                    .push((self.interval_start(interval), self.interval_size(interval)));
            }
        }
        start_durations_pairs.sort_unstable();
        let mut previous_end = i64::MIN;
        for pair in &start_durations_pairs {
            if pair.0 < previous_end {
                return false;
            }
            previous_end = pair.0 + pair.1;
        }
        true
    }

    fn no_overlap_2d_constraint_is_feasible(
        &self,
        model: &CpModelProto,
        arg: &NoOverlap2DConstraintProto,
    ) -> bool {
        // Those intervals from arg.x_intervals and arg.y_intervals where both
        // the x and y intervals are enforced.
        let mut has_zero_sizes = false;
        let mut enforced_rectangles: Vec<Rectangle> = Vec::new();
        {
            let num_intervals = arg.x_intervals.len();
            assert_eq!(arg.y_intervals.len(), num_intervals);
            for i in 0..num_intervals {
                let x = &model.constraints[arg.x_intervals[i] as usize];
                let y = &model.constraints[arg.y_intervals[i] as usize];
                if self.constraint_is_enforced(x) && self.constraint_is_enforced(y) {
                    enforced_rectangles.push(Rectangle {
                        x_min: IntegerValue::new(self.interval_start(interval_of(x))),
                        x_max: IntegerValue::new(self.interval_end(interval_of(x))),
                        y_min: IntegerValue::new(self.interval_start(interval_of(y))),
                        y_max: IntegerValue::new(self.interval_end(interval_of(y))),
                    });
                    let rect = enforced_rectangles
                        .last()
                        .expect("a rectangle was just pushed");
                    if rect.x_min == rect.x_max || rect.y_min == rect.y_max {
                        has_zero_sizes = true;
                    }
                }
            }
        }

        // A stable sort on `x_min`, as `absl::c_stable_sort` does.
        enforced_rectangles.sort_by_key(|rectangle| rectangle.x_min.value());
        let one_intersection: Option<(i32, i32)> = if has_zero_sizes {
            find_one_intersection_if_present_with_zero_area(&enforced_rectangles)
        } else {
            find_one_intersection_if_present(&enforced_rectangles)
        };

        if let Some(one_intersection) = one_intersection {
            log::debug!(
                "Rectangles {}({}) and {}({}) are not disjoint.",
                one_intersection.0,
                enforced_rectangles[one_intersection.0 as usize],
                one_intersection.1,
                enforced_rectangles[one_intersection.1 as usize]
            );
            return false;
        }
        true
    }

    fn cumulative_constraint_is_feasible(
        &self,
        model: &CpModelProto,
        arg: &CumulativeConstraintProto,
    ) -> bool {
        let capacity = self.linear_expression_value(or_empty(&arg.capacity));
        if capacity < 0 {
            return false;
        }
        let num_intervals = arg.intervals.len();
        let mut events: Vec<(i64, i64)> = Vec::new();
        for i in 0..num_intervals {
            let interval_constraint = &model.constraints[arg.intervals[i] as usize];
            if !self.constraint_is_enforced(interval_constraint) {
                continue;
            }
            let start = self.interval_start(interval_of(interval_constraint));
            let duration = self.interval_size(interval_of(interval_constraint));
            let demand = self.linear_expression_value(&arg.demands[i]);
            if duration == 0 || demand == 0 {
                continue;
            }
            events.push((start, demand));
            events.push((start + duration, -demand));
        }
        if events.is_empty() {
            return true;
        }

        events.sort_unstable();

        // This works because we will process negative demands first.
        let mut current_load: i64 = 0;
        for &(time, delta) in &events {
            current_load += delta;
            if current_load > capacity {
                log::debug!(
                    "Cumulative constraint: load: {current_load} capacity: {capacity} time: {time}"
                );
                for i in 0..num_intervals {
                    let interval_constraint = &model.constraints[arg.intervals[i] as usize];
                    if !self.constraint_is_enforced(interval_constraint) {
                        log::debug!("Interval : {} not enforced", arg.intervals[i]);
                    } else {
                        let start = self.interval_start(interval_of(interval_constraint));
                        let duration = self.interval_size(interval_of(interval_constraint));
                        let demand = self.linear_expression_value(&arg.demands[i]);
                        log::debug!(
                            "Interval : {} [{start},{}] demand={demand}",
                            arg.intervals[i],
                            start + duration
                        );
                    }
                }
                return false;
            }
        }
        debug_assert_eq!(current_load, 0);
        true
    }

    fn element_constraint_is_feasible(&self, arg: &ElementConstraintProto) -> bool {
        if !arg.vars.is_empty() {
            let index = self.value(arg.index) as i32;
            if index < 0 || index >= arg.vars.len() as i32 {
                return false;
            }
            return self.value(arg.vars[index as usize]) == self.value(arg.target);
        }

        if !arg.exprs.is_empty() {
            let index = self.linear_expression_value(or_empty(&arg.linear_index)) as i32;
            if index < 0 || index >= arg.exprs.len() as i32 {
                return false;
            }
            return self.linear_expression_value(&arg.exprs[index as usize])
                == self.linear_expression_value(or_empty(&arg.linear_target));
        }

        false
    }

    fn table_constraint_is_feasible(&self, arg: &TableConstraintProto) -> bool {
        let mut solution: Vec<i64> = Vec::new();
        if arg.exprs.is_empty() {
            for i in 0..arg.vars.len() {
                solution.push(self.value(arg.vars[i]));
            }
        } else {
            for i in 0..arg.exprs.len() {
                solution.push(self.linear_expression_value(&arg.exprs[i]));
            }
        }

        // No expression -> always feasible.
        if solution.is_empty() {
            return true;
        }
        let size = solution.len();

        for row_start in (0..arg.values.len()).step_by(size) {
            let mut i = 0;
            while solution[i] == arg.values[row_start + i] {
                i += 1;
                if i == size {
                    return !arg.negated;
                }
            }
        }
        arg.negated
    }

    fn automaton_constraint_is_feasible(&self, automaton: &AutomatonConstraintProto) -> bool {
        // Build the transition table {tail, label} -> head.
        let mut transition_map: HashMap<(i64, i64), i64> = HashMap::new();
        let num_transitions = automaton.transition_tail.len();
        for i in 0..num_transitions {
            transition_map.insert(
                (automaton.transition_tail[i], automaton.transition_label[i]),
                automaton.transition_head[i],
            );
        }

        // Walk the automaton.
        let mut current_state: i64 = automaton.starting_state;
        let num_steps = automaton.vars.len().max(automaton.exprs.len());
        for i in 0..num_steps {
            let key: (i64, i64) = (
                current_state,
                if automaton.vars.is_empty() {
                    self.linear_expression_value(&automaton.exprs[i])
                } else {
                    self.value(automaton.vars[i])
                },
            );
            let Some(&head) = transition_map.get(&key) else {
                return false;
            };
            current_state = head;
        }

        // Check we are now in a final state.
        for &r#final in &automaton.final_states {
            if current_state == r#final {
                return true;
            }
        }
        false
    }

    fn circuit_constraint_is_feasible(&self, circuit: &CircuitConstraintProto) -> bool {
        // Compute the set of relevant nodes for the constraint and set the next of
        // each of them. This also detects duplicate nexts.
        let num_arcs = circuit.tails.len();
        let mut nodes: HashSet<i32> = HashSet::new();
        let mut nexts: HashMap<i32, i32> = HashMap::new();
        for i in 0..num_arcs {
            let tail = circuit.tails[i];
            let head = circuit.heads[i];
            nodes.insert(tail);
            nodes.insert(head);
            if self.literal_is_false(circuit.literals[i]) {
                continue;
            }
            if nexts.contains_key(&tail) {
                log::debug!("Node with two outgoing arcs");
                return false; // Duplicate.
            }
            nexts.insert(tail, head);
        }

        // All nodes must have a next. The result does not depend on the order of the
        // hash set (sat/cp_model_checker.cc:1756): `in_cycle` is any node that is not a
        // self-loop, and the walk below from it is a cycle that holds them all or not.
        let mut in_cycle: i32 = 0;
        let mut cycle_size = 0;
        for &node in &nodes {
            let Some(&next) = nexts.get(&node) else {
                log::debug!("Node with no next: {node}");
                return false; // No next.
            };
            if next == node {
                continue; // skip self-loop.
            }
            in_cycle = node;
            cycle_size += 1;
        }
        if cycle_size == 0 {
            return true;
        }

        // Check that we have only one cycle. visited is used to not loop forever if
        // we have a "rho" shape instead of a cycle.
        let mut visited: HashSet<i32> = HashSet::new();
        let mut current = in_cycle;
        let mut num_visited = 0;
        while !visited.contains(&current) {
            num_visited += 1;
            visited.insert(current);
            current = nexts[&current];
        }
        if current != in_cycle {
            log::debug!("Rho shape");
            return false; // Rho shape.
        }
        if num_visited != cycle_size {
            log::debug!("More than one cycle");
        }
        num_visited == cycle_size // Another cycle somewhere if false.
    }

    fn routes_constraint_is_feasible(&self, routes: &RoutesConstraintProto) -> bool {
        let num_arcs = routes.tails.len();
        let mut num_used_arcs = 0;
        let mut num_self_arcs = 0;

        // Compute the number of nodes.
        let mut num_nodes: i32 = 0;
        for i in 0..num_arcs {
            num_nodes = num_nodes.max(1 + routes.tails[i]);
            num_nodes = num_nodes.max(1 + routes.heads[i]);
        }

        let mut tail_to_head: Vec<i32> = vec![-1; num_nodes as usize];
        let mut has_incoming_arc: Vec<bool> = vec![false; num_nodes as usize];
        let mut has_outgoing_arc: Vec<bool> = vec![false; num_nodes as usize];
        let mut depot_nexts: Vec<i32> = Vec::new();
        for i in 0..num_arcs {
            let tail = routes.tails[i];
            let head = routes.heads[i];
            if self.literal_is_true(routes.literals[i]) {
                // Check for loops.
                if tail != 0 {
                    if has_outgoing_arc[tail as usize] {
                        log::debug!("routes: node {tail} has two outgoing arcs");
                        return false;
                    }
                    has_outgoing_arc[tail as usize] = true;
                }
                if head != 0 {
                    if has_incoming_arc[head as usize] {
                        log::debug!("routes: node {head} has two incoming arcs");
                        return false;
                    }
                    has_incoming_arc[head as usize] = true;
                }

                if tail == head {
                    if tail == 0 {
                        log::debug!("Self loops on node 0 are forbidden.");
                        return false;
                    }
                    num_self_arcs += 1;
                    continue;
                }
                num_used_arcs += 1;
                if tail == 0 {
                    depot_nexts.push(head);
                } else {
                    debug_assert_eq!(tail_to_head[tail as usize], -1);
                    tail_to_head[tail as usize] = head;
                }
            }
        }

        // An empty constraint with no node to visit should be feasible.
        if num_nodes == 0 {
            return true;
        }

        // Make sure each route from the depot goes back to it, and count such arcs.
        let mut count = 0;
        for &depot_next in &depot_nexts {
            let mut start = depot_next;
            count += 1;
            while start != 0 {
                if tail_to_head[start as usize] == -1 {
                    return false;
                }
                start = tail_to_head[start as usize];
                count += 1;
            }
        }

        if count != num_used_arcs {
            log::debug!("count: {count} != num_used_arcs:{num_used_arcs}");
            return false;
        }

        // The routes cover as many nodes as there are arcs, but this way we count
        // multiple times the depot. So the number of nodes covered is:
        //     count - depot_nexts.size() + 1.
        // And this number + the self arcs should be num_nodes.
        if count as usize - depot_nexts.len() + 1 + num_self_arcs as usize != num_nodes as usize {
            log::debug!("Not all nodes are covered!");
            return false;
        }

        true
    }

    fn inverse_constraint_is_feasible(&self, inverse: &InverseConstraintProto) -> bool {
        let num_variables = inverse.f_direct.len() as i32;
        if num_variables != inverse.f_inverse.len() as i32 {
            return false;
        }
        // Check that f_inverse(f_direct(i)) == i; this is sufficient.
        for i in 0..num_variables {
            let fi = self.value(inverse.f_direct[i as usize]) as i32;
            if fi < 0 || num_variables <= fi {
                return false;
            }
            if i64::from(i) != self.value(inverse.f_inverse[fi as usize]) {
                return false;
            }
        }
        let num_exprs = inverse.f_expr_direct.len() as i32;
        if num_exprs != inverse.f_expr_inverse.len() as i32 {
            return false;
        }
        // Check that f_expr_inverse(f_expr_direct(i)) == i; this is sufficient.
        for i in 0..num_exprs {
            let fi = self.linear_expression_value(&inverse.f_expr_direct[i as usize]) as i32;
            if fi < 0 || num_exprs <= fi {
                return false;
            }
            if i64::from(i) != self.linear_expression_value(&inverse.f_expr_inverse[fi as usize]) {
                return false;
            }
        }
        true
    }

    fn reservoir_constraint_is_feasible(&self, reservoir: &ReservoirConstraintProto) -> bool {
        let num_variables = reservoir.time_exprs.len();
        let min_level: i64 = reservoir.min_level;
        let max_level: i64 = reservoir.max_level;
        let mut deltas: BTreeMap<i64, i64> = BTreeMap::new();
        let has_active_variables = !reservoir.active_literals.is_empty();
        for i in 0..num_variables {
            let time = self.linear_expression_value(&reservoir.time_exprs[i]);
            if !has_active_variables || self.literal_is_true(reservoir.active_literals[i]) {
                let level = self.linear_expression_value(&reservoir.level_changes[i]);
                *deltas.entry(time).or_insert(0) += level;
            }
        }
        let mut current_level: i64 = 0;
        for delta in &deltas {
            current_level += delta.1;
            if current_level < min_level || current_level > max_level {
                log::debug!(
                    "Reservoir level {current_level} is out of bounds at time: {}",
                    delta.0
                );
                return false;
            }
        }
        true
    }

    fn constraint_is_feasible(&self, model: &CpModelProto, ct: &ConstraintProto) -> bool {
        // A non-enforced constraint is always feasible.
        if !self.constraint_is_enforced(ct) {
            return true;
        }

        match &ct.constraint {
            Some(Constraint::BoolOr(arg)) => self.bool_or_constraint_is_feasible(arg),
            Some(Constraint::BoolAnd(arg)) => self.bool_and_constraint_is_feasible(arg),
            Some(Constraint::AtMostOne(arg)) => self.at_most_one_constraint_is_feasible(arg),
            Some(Constraint::ExactlyOne(arg)) => self.exactly_one_constraint_is_feasible(arg),
            Some(Constraint::BoolXor(arg)) => self.bool_xor_constraint_is_feasible(arg),
            Some(Constraint::Linear(arg)) => self.linear_constraint_is_feasible(arg),
            Some(Constraint::IntProd(arg)) => self.int_prod_constraint_is_feasible(arg),
            Some(Constraint::IntDiv(arg)) => self.int_div_constraint_is_feasible(arg),
            Some(Constraint::IntMod(arg)) => self.int_mod_constraint_is_feasible(arg),
            Some(Constraint::LinMax(arg)) => self.lin_max_constraint_is_feasible(arg),
            Some(Constraint::AllDiff(arg)) => self.all_diff_constraint_is_feasible(arg),
            Some(Constraint::Interval(arg)) => {
                if !self.interval_constraint_is_feasible(arg) {
                    if arg.start.is_some() {
                        // Tricky: For simplified presolve, we require that a separate
                        // constraint is added to the model to enforce the "interval".
                        // This indicates that such a constraint was not added to the model.
                        // It should probably be a validation error, but it is hard to
                        // detect beforehand.
                        log::debug!(
                            "Warning, an interval constraint was likely used \
                             without a corresponding linear constraint linking \
                             its start, size and end."
                        );
                    }
                    return false;
                }
                true
            }
            Some(Constraint::NoOverlap(arg)) => self.no_overlap_constraint_is_feasible(model, arg),
            Some(Constraint::NoOverlap2d(arg)) => {
                self.no_overlap_2d_constraint_is_feasible(model, arg)
            }
            Some(Constraint::Cumulative(arg)) => self.cumulative_constraint_is_feasible(model, arg),
            Some(Constraint::Element(arg)) => self.element_constraint_is_feasible(arg),
            Some(Constraint::Table(arg)) => self.table_constraint_is_feasible(arg),
            Some(Constraint::Automaton(arg)) => self.automaton_constraint_is_feasible(arg),
            Some(Constraint::Circuit(arg)) => self.circuit_constraint_is_feasible(arg),
            Some(Constraint::Routes(arg)) => self.routes_constraint_is_feasible(arg),
            Some(Constraint::Inverse(arg)) => self.inverse_constraint_is_feasible(arg),
            Some(Constraint::Reservoir(arg)) => self.reservoir_constraint_is_feasible(arg),
            // Empty constraint is always feasible.
            None => true,
            Some(Constraint::DummyConstraint(_)) => {
                panic!(
                    "Unsupported constraint: {}",
                    constraint_case_name(constraint_case(ct))
                );
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use cpsat_proto::CpModelProto;
    use cpsat_testutil::{expect_death, parse_test_proto};

    use super::*;

    // `HasSubstr` of gMock.
    fn assert_has_substr(actual: &str, expected: &str) {
        assert!(
            actual.contains(expected),
            "{actual:?} does not contain {expected:?}"
        );
    }

    #[test]
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

    // The cases below are not upstream cases: they check the constraint kinds and the
    // validation branches that the upstream cases leave out.

    fn model_of(text: &str) -> CpModelProto {
        parse_test_proto(text)
    }

    #[test]
    fn solution_is_feasible_rejects_a_wrong_number_of_values_and_a_value_outside_its_domain() {
        // Arrange
        let model = model_of("variables { domain: [0, 10] }");

        // Act and Assert
        assert!(!solution_is_feasible(&model, &[1, 2], None, None));
        assert!(!solution_is_feasible(&model, &[11], None, None));
        assert!(solution_is_feasible(&model, &[10], None, None));
    }

    #[test]
    fn solution_is_feasible_checks_bool_or_and_bool_and() {
        // Arrange
        let bool_or = model_of(
            r#"
            variables { domain: [0, 1] }
            variables { domain: [0, 1] }
            constraints { bool_or { literals: [0, -2] } }
            "#,
        );
        let bool_and = model_of(
            r#"
            variables { domain: [0, 1] }
            variables { domain: [0, 1] }
            constraints { bool_and { literals: [0, -2] } }
            "#,
        );

        // Act and Assert
        assert!(solution_is_feasible(&bool_or, &[1, 1], None, None));
        assert!(solution_is_feasible(&bool_or, &[0, 0], None, None));
        assert!(!solution_is_feasible(&bool_or, &[0, 1], None, None));
        assert!(solution_is_feasible(&bool_and, &[1, 0], None, None));
        assert!(!solution_is_feasible(&bool_and, &[1, 1], None, None));
        assert!(!solution_is_feasible(&bool_and, &[0, 0], None, None));
    }

    #[test]
    fn solution_is_feasible_checks_at_most_one_and_exactly_one() {
        // Arrange
        let at_most_one = model_of(
            r#"
            variables { domain: [0, 1] }
            variables { domain: [0, 1] }
            variables { domain: [0, 1] }
            constraints { at_most_one { literals: [0, 1, 2] } }
            "#,
        );
        let exactly_one = model_of(
            r#"
            variables { domain: [0, 1] }
            variables { domain: [0, 1] }
            variables { domain: [0, 1] }
            constraints { exactly_one { literals: [0, 1, 2] } }
            "#,
        );

        // Act and Assert
        assert!(solution_is_feasible(&at_most_one, &[0, 0, 0], None, None));
        assert!(solution_is_feasible(&at_most_one, &[0, 1, 0], None, None));
        assert!(!solution_is_feasible(&at_most_one, &[1, 1, 0], None, None));
        assert!(!solution_is_feasible(&exactly_one, &[0, 0, 0], None, None));
        assert!(solution_is_feasible(&exactly_one, &[0, 0, 1], None, None));
        assert!(!solution_is_feasible(&exactly_one, &[1, 1, 0], None, None));
    }

    #[test]
    fn solution_is_feasible_checks_int_prod() {
        // Arrange
        let model = model_of(
            r#"
            variables { domain: [0, 10] }
            variables { domain: [0, 10] }
            variables { domain: [0, 100] }
            constraints {
              int_prod {
                target { vars: 2 coeffs: 1 }
                exprs { vars: 0 coeffs: 1 }
                exprs { vars: 1 coeffs: 1 }
              }
            }
            "#,
        );

        // Act and Assert
        assert!(solution_is_feasible(&model, &[2, 3, 6], None, None));
        assert!(!solution_is_feasible(&model, &[2, 3, 5], None, None));
    }

    #[test]
    fn solution_is_feasible_checks_int_div_and_int_mod() {
        // Arrange
        let int_div = model_of(
            r#"
            variables { domain: [0, 10] }
            variables { domain: [1, 10] }
            variables { domain: [0, 10] }
            constraints {
              int_div {
                target { vars: 2 coeffs: 1 }
                exprs { vars: 0 coeffs: 1 }
                exprs { vars: 1 coeffs: 1 }
              }
            }
            "#,
        );
        let int_mod = model_of(
            r#"
            variables { domain: [0, 10] }
            variables { domain: [1, 10] }
            variables { domain: [0, 10] }
            constraints {
              int_mod {
                target { vars: 2 coeffs: 1 }
                exprs { vars: 0 coeffs: 1 }
                exprs { vars: 1 coeffs: 1 }
              }
            }
            "#,
        );

        // Act and Assert
        assert!(solution_is_feasible(&int_div, &[7, 2, 3], None, None));
        assert!(!solution_is_feasible(&int_div, &[7, 2, 4], None, None));
        assert!(solution_is_feasible(&int_mod, &[7, 2, 1], None, None));
        assert!(!solution_is_feasible(&int_mod, &[7, 2, 0], None, None));
    }

    #[test]
    fn solution_is_feasible_checks_all_diff() {
        // Arrange
        let model = model_of(
            r#"
            variables { domain: [0, 10] }
            variables { domain: [0, 10] }
            variables { domain: [0, 10] }
            constraints {
              all_diff {
                exprs { vars: 0 coeffs: 1 }
                exprs { vars: 1 coeffs: 1 }
                exprs { vars: 2 coeffs: 1 offset: 1 }
              }
            }
            "#,
        );

        // Act and Assert
        assert!(solution_is_feasible(&model, &[1, 2, 3], None, None));
        assert!(!solution_is_feasible(&model, &[1, 2, 1], None, None));
    }

    #[test]
    fn solution_is_feasible_checks_the_start_size_and_end_of_an_interval() {
        // Arrange
        let model = model_of(
            r#"
            variables { domain: [0, 10] }
            variables { domain: [0, 10] }
            variables { domain: [-1, 2] }
            constraints {
              interval {
                start { vars: 0 coeffs: 1 }
                size { vars: 2 coeffs: 1 }
                end { vars: 1 coeffs: 1 }
              }
            }
            "#,
        );

        // Act and Assert
        assert!(solution_is_feasible(&model, &[1, 3, 2], None, None));
        assert!(!solution_is_feasible(&model, &[1, 4, 2], None, None));
        assert!(!solution_is_feasible(&model, &[1, 0, -1], None, None));
    }

    #[test]
    fn solution_is_feasible_checks_no_overlap() {
        // Arrange
        let model = model_of(
            r#"
            variables { domain: [0, 1] }
            variables { domain: [0, 10] }
            variables { domain: [0, 10] }
            constraints {
              interval {
                start { vars: 1 coeffs: 1 }
                size { offset: 3 }
                end { vars: 1 coeffs: 1 offset: 3 }
              }
            }
            constraints {
              enforcement_literal: 0
              interval {
                start { vars: 2 coeffs: 1 }
                size { offset: 3 }
                end { vars: 2 coeffs: 1 offset: 3 }
              }
            }
            constraints { no_overlap { intervals: [0, 1] } }
            "#,
        );

        // Act and Assert
        assert!(!solution_is_feasible(&model, &[1, 0, 2], None, None));
        assert!(solution_is_feasible(&model, &[1, 0, 3], None, None));
        assert!(solution_is_feasible(&model, &[0, 0, 2], None, None));
    }

    #[test]
    fn solution_is_feasible_checks_no_overlap_2d_with_and_without_zero_sizes() {
        // Arrange
        let boxes = model_of(
            r#"
            variables { domain: [0, 4] }
            constraints {
              interval {
                start { offset: 0 }
                size { offset: 2 }
                end { offset: 2 }
              }
            }
            constraints {
              interval {
                start { vars: 0 coeffs: 1 }
                size { offset: 2 }
                end { vars: 0 coeffs: 1 offset: 2 }
              }
            }
            constraints {
              interval {
                start { offset: 0 }
                size { offset: 2 }
                end { offset: 2 }
              }
            }
            constraints { no_overlap_2d { x_intervals: [0, 1] y_intervals: [2, 2] } }
            "#,
        );
        let line_in_box = model_of(
            r#"
            variables { domain: [0, 4] }
            constraints {
              interval {
                start { offset: 0 }
                size { offset: 2 }
                end { offset: 2 }
              }
            }
            constraints {
              interval {
                start { vars: 0 coeffs: 1 }
                size { offset: 0 }
                end { vars: 0 coeffs: 1 }
              }
            }
            constraints {
              interval {
                start { offset: 0 }
                size { offset: 2 }
                end { offset: 2 }
              }
            }
            constraints { no_overlap_2d { x_intervals: [0, 1] y_intervals: [2, 2] } }
            "#,
        );

        // Act and Assert
        assert!(!solution_is_feasible(&boxes, &[1], None, None));
        assert!(solution_is_feasible(&boxes, &[2], None, None));
        assert!(!solution_is_feasible(&line_in_box, &[1], None, None));
        assert!(solution_is_feasible(&line_in_box, &[4], None, None));
    }

    #[test]
    fn solution_is_feasible_checks_the_load_and_the_capacity_of_a_cumulative() {
        // Arrange
        let model = model_of(
            r#"
            variables { domain: [0, 4] }
            variables { domain: [-2, 1] }
            variables { domain: [0, 1] }
            constraints {
              interval {
                start { vars: 0 coeffs: 1 }
                size { offset: 2 }
                end { vars: 0 coeffs: 1 offset: 2 }
              }
            }
            constraints {
              enforcement_literal: 2
              interval {
                start { offset: 0 }
                size { offset: 2 }
                end { offset: 2 }
              }
            }
            constraints {
              interval {
                start { offset: 0 }
                size { offset: 0 }
                end { offset: 0 }
              }
            }
            constraints {
              cumulative {
                capacity { vars: 1 coeffs: 1 offset: 1 }
                intervals: [0, 1, 2]
                demands { offset: 1 }
                demands { offset: 1 }
                demands { offset: 1 }
              }
            }
            "#,
        );

        // Act and Assert
        assert!(!solution_is_feasible(&model, &[0, 0, 1], None, None));
        assert!(solution_is_feasible(&model, &[0, 0, 0], None, None));
        assert!(solution_is_feasible(&model, &[0, 1, 1], None, None));
        assert!(solution_is_feasible(&model, &[2, 0, 1], None, None));
        assert!(!solution_is_feasible(&model, &[0, -1, 1], None, None));
        assert!(!solution_is_feasible(&model, &[0, -2, 1], None, None));
    }

    #[test]
    fn solution_is_feasible_checks_element_in_both_formats() {
        // Arrange
        let legacy = model_of(
            r#"
            variables { domain: [-1, 2] }
            variables { domain: [0, 10] }
            variables { domain: [0, 10] }
            variables { domain: [0, 10] }
            constraints { element { index: 0 target: 1 vars: [2, 3] } }
            "#,
        );
        let linear = model_of(
            r#"
            variables { domain: [-1, 2] }
            variables { domain: [0, 10] }
            variables { domain: [0, 10] }
            variables { domain: [0, 10] }
            constraints {
              element {
                linear_index { vars: 0 coeffs: 1 }
                linear_target { vars: 1 coeffs: 1 }
                exprs { vars: 2 coeffs: 1 }
                exprs { vars: 3 coeffs: 1 }
              }
            }
            "#,
        );
        let empty = model_of("constraints { element {} }");

        // Act and Assert
        assert!(solution_is_feasible(&legacy, &[1, 7, 5, 7], None, None));
        assert!(!solution_is_feasible(&legacy, &[1, 7, 7, 5], None, None));
        assert!(!solution_is_feasible(&legacy, &[2, 7, 7, 7], None, None));
        assert!(!solution_is_feasible(&legacy, &[-1, 7, 7, 7], None, None));
        assert!(solution_is_feasible(&linear, &[0, 5, 5, 7], None, None));
        assert!(!solution_is_feasible(&linear, &[0, 5, 7, 5], None, None));
        assert!(!solution_is_feasible(&linear, &[2, 5, 5, 5], None, None));
        assert!(!solution_is_feasible(&linear, &[-1, 5, 5, 5], None, None));
        assert!(!solution_is_feasible(&empty, &[], None, None));
    }

    #[test]
    fn solution_is_feasible_checks_table_and_negated_table() {
        // Arrange
        let table = model_of(
            r#"
            variables { domain: [0, 5] }
            variables { domain: [0, 5] }
            constraints { table { vars: [0, 1] values: [1, 2, 3, 4] } }
            "#,
        );
        let negated = model_of(
            r#"
            variables { domain: [0, 5] }
            variables { domain: [0, 5] }
            constraints { table { vars: [0, 1] values: [1, 2, 3, 4] negated: true } }
            "#,
        );
        let expressions = model_of(
            r#"
            variables { domain: [0, 5] }
            constraints {
              table {
                exprs { vars: 0 coeffs: 1 offset: 1 }
                values: [1, 2]
              }
            }
            "#,
        );
        let no_expression = model_of("constraints { table {} }");

        // Act and Assert
        assert!(solution_is_feasible(&table, &[3, 4], None, None));
        assert!(!solution_is_feasible(&table, &[1, 3], None, None));
        assert!(!solution_is_feasible(&negated, &[3, 4], None, None));
        assert!(solution_is_feasible(&negated, &[1, 3], None, None));
        assert!(solution_is_feasible(&expressions, &[0], None, None));
        assert!(solution_is_feasible(&expressions, &[1], None, None));
        assert!(!solution_is_feasible(&expressions, &[2], None, None));
        assert!(solution_is_feasible(&no_expression, &[], None, None));
    }

    #[test]
    fn solution_is_feasible_checks_automaton_with_variables_and_with_expressions() {
        // Arrange
        let with_variables = model_of(
            r#"
            variables { domain: [0, 1] }
            variables { domain: [0, 1] }
            constraints {
              automaton {
                starting_state: 1
                final_states: 3
                transition_tail: [1, 2]
                transition_head: [2, 3]
                transition_label: [0, 1]
                vars: [0, 1]
              }
            }
            "#,
        );
        let with_expressions = model_of(
            r#"
            variables { domain: [0, 1] }
            constraints {
              automaton {
                starting_state: 1
                final_states: 2
                transition_tail: [1]
                transition_head: [2]
                transition_label: [1]
                exprs { vars: 0 coeffs: 1 }
              }
            }
            "#,
        );

        // Act and Assert
        assert!(solution_is_feasible(&with_variables, &[0, 1], None, None));
        assert!(!solution_is_feasible(&with_variables, &[0, 0], None, None));
        assert!(solution_is_feasible(&with_expressions, &[1], None, None));
        assert!(!solution_is_feasible(&with_expressions, &[0], None, None));
    }

    #[test]
    fn solution_is_feasible_checks_circuit_shapes() {
        // Arrange
        let literals = "variables { domain: [0, 1] }".repeat(4);
        let one_cycle = model_of(&format!(
            "{literals} constraints {{ circuit {{ tails: [0, 1, 2] heads: [1, 2, 0] literals: [0, 1, 2] }} }}"
        ));
        let two_cycles = model_of(&format!(
            "{literals} constraints {{ circuit {{ tails: [0, 1, 2, 3] heads: [1, 0, 3, 2] literals: [0, 1, 2, 3] }} }}"
        ));
        let two_outgoing_arcs = model_of(&format!(
            "{literals} constraints {{ circuit {{ tails: [0, 0, 1] heads: [1, 2, 0] literals: [0, 1, 2] }} }}"
        ));
        let missing_next = model_of(&format!(
            "{literals} constraints {{ circuit {{ tails: [0, 1] heads: [1, 0] literals: [0, 1] }} }}"
        ));
        let rho = model_of(&format!(
            "{literals} constraints {{ circuit {{ tails: [0, 1, 2] heads: [1, 2, 1] literals: [0, 1, 2] }} }}"
        ));

        // Act and Assert
        assert!(solution_is_feasible(&one_cycle, &[1, 1, 1, 0], None, None));
        assert!(!solution_is_feasible(
            &two_cycles,
            &[1, 1, 1, 1],
            None,
            None
        ));
        assert!(!solution_is_feasible(
            &two_outgoing_arcs,
            &[1, 1, 1, 0],
            None,
            None
        ));
        assert!(!solution_is_feasible(
            &missing_next,
            &[1, 0, 0, 0],
            None,
            None
        ));
        assert!(!solution_is_feasible(&rho, &[1, 1, 1, 0], None, None));
    }

    #[test]
    fn solution_is_feasible_checks_routes_shapes() {
        // Arrange
        let literals = "variables { domain: [0, 1] }".repeat(4);
        let routes = |tails: &str, heads: &str, literal_refs: &str| {
            model_of(&format!(
                "{literals} constraints {{ routes {{ tails: [{tails}] heads: [{heads}] literals: [{literal_refs}] }} }}"
            ))
        };
        let one_route = routes("0, 1, 2", "1, 2, 0", "0, 1, 2");
        let depot_self_loop = routes("0, 0", "0, 1", "0, 1");
        let two_outgoing_arcs = routes("0, 1, 1", "1, 2, 0", "0, 1, 2");
        let two_incoming_arcs = routes("0, 1, 2", "2, 2, 0", "0, 1, 2");
        let node_not_covered = routes("0, 1, 2", "1, 0, 2", "0, 1, 2");
        let self_arc_covers_node = routes("0, 1, 2", "1, 0, 2", "0, 1, 2");
        let route_apart_from_the_depot = routes("0, 1, 2, 3", "1, 0, 3, 2", "0, 1, 2, 3");
        let broken_route = routes("0, 2", "1, 3", "0, 1");
        let empty = routes("", "", "");

        // Act and Assert
        assert!(solution_is_feasible(&one_route, &[1, 1, 1, 0], None, None));
        assert!(!solution_is_feasible(
            &depot_self_loop,
            &[1, 0, 0, 0],
            None,
            None
        ));
        assert!(!solution_is_feasible(
            &two_outgoing_arcs,
            &[1, 1, 1, 0],
            None,
            None
        ));
        assert!(!solution_is_feasible(
            &two_incoming_arcs,
            &[1, 1, 1, 0],
            None,
            None
        ));
        assert!(!solution_is_feasible(
            &node_not_covered,
            &[1, 1, 0, 0],
            None,
            None
        ));
        assert!(solution_is_feasible(
            &self_arc_covers_node,
            &[1, 1, 1, 0],
            None,
            None
        ));
        assert!(!solution_is_feasible(
            &route_apart_from_the_depot,
            &[1, 1, 1, 1],
            None,
            None
        ));
        assert!(!solution_is_feasible(
            &broken_route,
            &[1, 1, 0, 0],
            None,
            None
        ));
        assert!(solution_is_feasible(&empty, &[0, 0, 0, 0], None, None));
    }

    #[test]
    fn solution_is_feasible_accepts_an_empty_constraint() {
        // Arrange
        let model = model_of("variables { domain: [0, 1] } constraints {}");

        // Act and Assert
        assert!(solution_is_feasible(&model, &[0], None, None));
    }

    #[test]
    fn solution_is_feasible_dies_on_the_dummy_constraint() {
        // Arrange
        let model = model_of("variables { domain: [0, 1] } constraints { dummy_constraint {} }");

        // Act and Assert
        expect_death(
            || {
                solution_is_feasible(&model, &[0], None, None);
            },
            "Unsupported constraint: kDummyConstraint",
        );
    }

    #[test]
    fn solution_is_feasible_walks_the_postsolve_mapping_of_a_failing_constraint() {
        // Arrange
        let model = model_of(
            r#"
            variables { domain: [0, 10] }
            constraints { linear { vars: 0 coeffs: 1 domain: [0, 3] } }
            "#,
        );
        let mapping_proto = model_of(
            r#"
            variables { domain: [0, 10] }
            variables { domain: [0, 10] }
            "#,
        );

        // Act and Assert
        assert!(!solution_is_feasible(
            &model,
            &[5],
            Some(&mapping_proto),
            Some(&[1])
        ));
        assert!(solution_is_feasible(
            &model,
            &[2],
            Some(&mapping_proto),
            Some(&[1])
        ));
    }

    #[test]
    fn solution_is_feasible_checks_an_objective_without_domain() {
        // Arrange
        let model = model_of(
            r#"
            variables { domain: [0, 10] }
            objective { vars: 0 coeffs: 2 offset: 1.5 scaling_factor: 2 }
            "#,
        );

        // Act and Assert
        assert!(solution_is_feasible(&model, &[4], None, None));
    }

    #[test]
    fn constraint_is_feasible_checks_one_constraint_against_the_model() {
        // Arrange
        let model = model_of(
            r#"
            variables { domain: [0, 10] }
            constraints { linear { vars: 0 coeffs: 1 domain: [0, 3] } }
            "#,
        );

        // Act and Assert
        assert!(constraint_is_feasible(&model, &model.constraints[0], &[2]));
        assert!(!constraint_is_feasible(&model, &model.constraints[0], &[5]));
    }

    #[test]
    fn solution_can_be_optimal_is_true_and_checks_dependent_variables_when_the_flag_is_set() {
        // Arrange
        let model = model_of(
            r#"
            variables { domain: [0, 10] }
            variables { domain: [0, 10] }
            constraints { linear { vars: [0, 1] coeffs: [1, -1] domain: [0, 0] } }
            "#,
        );

        // Act and Assert
        assert!(solution_can_be_optimal(&model, &[3, 3]));
        CP_MODEL_CHECK_DEPENDENT_VARIABLES.store(true, Ordering::Relaxed);
        let with_flag = solution_can_be_optimal(&model, &[3, 3]);
        let wrong_values = std::panic::catch_unwind(|| solution_can_be_optimal(&model, &[3, 4]));
        CP_MODEL_CHECK_DEPENDENT_VARIABLES.store(false, Ordering::Relaxed);
        assert!(with_flag);
        assert!(wrong_values.is_err());
    }

    // The base of the models of the validation tables: two integer variables and two Booleans.
    const FOUR_VARIABLES: &str = "variables { domain: [0, 10] } variables { domain: [0, 10] } \
                                  variables { domain: [0, 1] } variables { domain: [0, 1] } ";

    // A model with one variable that holds half of the range of an `int64_t`, on each side.
    const HALF_RANGE_VARIABLE: &str =
        "variables { domain: [-4611686018427387903, 4611686018427387903] } ";

    // One interval constraint with the constant size 2, for the models that need an interval to use.
    const INTERVAL: &str =
        "constraints { interval { start { offset: 0 } size { offset: 2 } end { offset: 2 } } } ";

    #[test]
    fn validate_cp_model_rejects_each_malformed_model() {
        // Arrange
        // Each row is: after_presolve, the model text, a part of the error.
        let cases: Vec<(bool, String, &str)> = vec![
            (false, "variables {}".into(), "has no domain()"),
            (false, "variables { domain: [5, 1] }".into(), "invalid domain() format"),
            (
                false,
                "variables { domain: [0, 4611686018427387903] }".repeat(3),
                "The sum of all variable domains does not fit",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ linear {{ vars: 7 coeffs: 1 domain: [0, 1] }} }}"),
                "Out of bound integer variable 7 in constraint #0",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ enforcement_literal: 0 bool_or {{ literals: 2 }} }}"),
                "Invalid enforcement literal 0 in constraint #0",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ bool_or {{ literals: 0 }} }}"),
                "Invalid literal 0 in constraint #0",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ linear {{ vars: -1 coeffs: 1 domain: [0, 1] }} }}"),
                "Invalid negated variable in linear constraint",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ linear {{ vars: 0 domain: [0, 1] }} }}"),
                "coeffs_size() != vars_size() in constraint",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ linear {{ vars: 0 coeffs: 9223372036854775807 domain: [0, 1] }} }}"),
                "Possible integer overflow in constraint",
            ),
            (
                true,
                format!("{FOUR_VARIABLES} constraints {{ linear {{ vars: 0 coeffs: 1 domain: [-9223372036854775808, 9223372036854775807] }} }}"),
                "Linear constraint domain was not tightenned",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ lin_max {{ target {{ vars: -1 coeffs: 1 }} }} }}"),
                "Invalid negated variable in linear expression",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ lin_max {{ target {{ vars: 0 coeffs: 1 }} exprs {{ vars: -1 coeffs: 1 }} }} }}"),
                "Invalid negated variable in linear expression",
            ),
            (
                true,
                format!("{FOUR_VARIABLES} constraints {{ enforcement_literal: [3, 2] lin_max {{ target {{ vars: 0 coeffs: 1 }} }} }}"),
                "LinMax constraint is not in canonical form",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ int_prod {{ exprs {{ vars: 0 coeffs: 1 }} }} }}"),
                "An int_prod constraint should have a target",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ int_prod {{ target {{}} exprs {{ offset: 4000000000 }} exprs {{ offset: 4000000000 }} }} }}"),
                "integer overflow in constraint",
            ),
            (
                false,
                "variables { domain: [0, 3000000] } constraints { int_prod { target {} exprs { vars: 0 coeffs: 1 } exprs { vars: 0 coeffs: 1 } exprs { vars: 0 coeffs: 1 } } }".into(),
                "Potential integer overflow in constraint",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ int_div {{ exprs {{}} exprs {{ offset: 1 }} }} }}"),
                "An int_div constraint should have a target",
            ),
            (
                false,
                "variables { domain: [2, 2] } variables { domain: [0, 10] } constraints { int_div { target {} exprs { vars: 1 coeffs: 1 } exprs { vars: 0 coeffs: 3 offset: -6 } } }".into(),
                "Division by 0",
            ),
            (
                false,
                "variables { domain: [-1, 1] } variables { domain: [-1, 1] } variables { domain: [-1, 1] } constraints { int_div { target {} exprs {} exprs { vars: [0, 1, 2] coeffs: [1, 1, 1] } } }".into(),
                "The domain of the divisor cannot contain 0",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ int_mod {{ exprs {{}} exprs {{ offset: 1 }} }} }}"),
                "An int_mod constraint should have a target",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ element {{ index: 0 vars: 1 linear_index {{ vars: 0 coeffs: 1 }} }} }}"),
                "Inconsistent element with both legacy and new format defined",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ element {{}} }}"),
                "Empty element constraint is interpreted as vars[]",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ element {{ index: 0 target: 1 vars: -1 }} }}"),
                "Element vars must be valid variables",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ element {{ vars: 0 index: 1 target: -1 }} }}"),
                "Element constraint index and target must be valid variables",
            ),
            (
                false,
                format!("{HALF_RANGE_VARIABLE} constraints {{ element {{ index: 0 target: 0 vars: 0 }} }}"),
                "Domain of the variables involved in element constraint may cause overflow",
            ),
            (
                false,
                format!("{HALF_RANGE_VARIABLE} constraints {{ element {{ linear_index {{ vars: 0 coeffs: 1 }} linear_target {{ vars: 0 coeffs: 1 }} exprs {{ vars: 0 coeffs: 1 }} }} }}"),
                "Domain of the variables involved in element constraint may cause overflow",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ inverse {{ f_direct: 9 f_inverse: 0 }} }}"),
                "Out of bound integer variable 9 in constraint #0",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ inverse {{ f_direct: -1 f_inverse: 0 }} }}"),
                "Invalid variable index in inverse constraint: -1",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ inverse {{ f_expr_direct {{ vars: -1 coeffs: 1 }} f_expr_inverse {{}} }} }}"),
                "Invalid negated variable in linear expression",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ table {{ vars: 0 exprs {{}} }} }}"),
                "Inconsistent table with both legacy and new format defined",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ table {{ values: 1 }} }}"),
                "Inconsistent table with empty expressions and non-empty tuples",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ table {{ vars: [0, 1] values: [1, 2, 3] }} }}"),
                "must be a multiple of the number of expressions",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ table {{ vars: -1 values: 1 }} }}"),
                "Invalid variable index in table constraint: -1",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ table {{ exprs {{ vars: -1 coeffs: 1 }} values: 1 }} }}"),
                "Invalid negated variable in linear expression",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ automaton {{ vars: 0 exprs {{}} }} }}"),
                "Inconsistent automaton with both legacy and new format defined",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ automaton {{ transition_tail: 0 }} }}"),
                "The transitions repeated fields must have the same size",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ automaton {{ vars: -1 }} }}"),
                "Invalid variable index in automaton constraint: -1",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ automaton {{ exprs {{ vars: -1 coeffs: 1 }} }} }}"),
                "Invalid negated variable in linear expression",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ automaton {{ transition_tail: 0 transition_head: 0 transition_label: 9223372036854775807 }} }}"),
                "labels in the automaton constraint are too big: 9223372036854775807",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ circuit {{ tails: 0 heads: 1 }} }}"),
                "Wrong field sizes in graph",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ circuit {{ tails: [0, 0] heads: [0, 0] literals: [2, 3] }} }}"),
                "Circuit/Route constraint contains multiple self-loops involving node 0",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ routes {{ tails: 0 heads: 0 literals: 2 }} }}"),
                "A route constraint cannot have a self-loop on the depot (node 0)",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ routes {{ tails: -1 heads: 0 literals: 2 }} }}"),
                "All nodes in a route constraint must be in [0, num_nodes)",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ routes {{ tails: 0 heads: -1 literals: 2 }} }}"),
                "All nodes in a route constraint must be in [0, num_nodes)",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ routes {{ tails: [0, 2] heads: [2, 0] literals: [2, 3] }} }}"),
                "All nodes in a route constraint must have incident arcs",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ interval {{ size {{}} end {{}} }} }}"),
                "Interval must have a start expression",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ interval {{ start {{}} end {{}} }} }}"),
                "Interval must have a size expression",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ interval {{ start {{}} size {{}} }} }}"),
                "Interval must have an end expression",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ interval {{ start {{ vars: [0, 1] coeffs: [1, 1] }} size {{}} end {{}} }} }}"),
                "Intervals with a start expression containing more than one variable",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ interval {{ start {{}} size {{ vars: [0, 1] coeffs: [1, 1] }} end {{}} }} }}"),
                "Intervals with a size expression containing more than one variable",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ interval {{ start {{}} size {{}} end {{ vars: [0, 1] coeffs: [1, 1] }} }} }}"),
                "Intervals with an end expression containing more than one variable",
            ),
            (
                false,
                format!("{HALF_RANGE_VARIABLE} constraints {{ interval {{ start {{ vars: 0 coeffs: 1 }} size {{}} end {{ vars: 0 coeffs: 1 }} }} }}"),
                "Possible overflow in interval",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} {INTERVAL} constraints {{ cumulative {{ intervals: 0 }} }}"),
                "intervals_size() != demands_size() in constraint",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} {INTERVAL} constraints {{ cumulative {{ intervals: 0 demands {{ offset: -1 }} }} }}"),
                "must be positive in constraint",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} {INTERVAL} constraints {{ cumulative {{ intervals: 0 demands {{ vars: [0, 1] coeffs: [1, 1] }} }} }}"),
                "must be affine or constant in constraint",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} {INTERVAL} constraints {{ cumulative {{ capacity {{ vars: [0, 1] coeffs: [1, 1] }} }} }}"),
                "must be affine or constant in constraint",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} {INTERVAL} constraints {{ cumulative {{ intervals: [0, 0, 0] demands {{ offset: 4611686018427387903 }} demands {{ offset: 4611686018427387903 }} demands {{ offset: 4611686018427387903 }} }} }}"),
                "The sum of max demands does not fit on an int64_t",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ no_overlap {{ intervals: 5 }} }}"),
                "Out of bound interval 5 in constraint #0",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ bool_or {{ literals: 2 }} }} constraints {{ no_overlap {{ intervals: 0 }} }}"),
                "Interval 0 does not refer to an interval constraint",
            ),
            (
                false,
                "constraints { interval { start {} size { offset: 4000000000 } end {} } } constraints { no_overlap_2d { x_intervals: 0 y_intervals: 0 } }".into(),
                "Integer overflow when summing all areas in constraint",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ reservoir {{ time_exprs {{}} }} }}"),
                "time_exprs and level_changes fields must be of the same size",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ reservoir {{ time_exprs {{ offset: 4611686018427387903 }} level_changes {{}} }} }}"),
                "Potential integer overflow on time_expr of a reservoir",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ reservoir {{ min_level: 1 }} }}"),
                "The min level of a reservoir must be <= 0",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ reservoir {{ max_level: -1 }} }}"),
                "The max level of a reservoir must be >= 0",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ reservoir {{ time_exprs {{}} time_exprs {{}} time_exprs {{}} level_changes {{ offset: 4611686018427387903 }} level_changes {{ offset: 4611686018427387903 }} level_changes {{ offset: 4611686018427387903 }} }} }}"),
                "Possible integer overflow in constraint",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ reservoir {{ time_exprs {{}} level_changes {{}} active_literals: [2, 3] }} }}"),
                "Wrong array length of active_literals variables",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} constraints {{ dummy_constraint {{}} }}"),
                "The dummy constraint should never appear in a model.",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} objective {{ domain: [5, 1] }}"),
                "The objective has an invalid domain() format",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} objective {{ vars: 0 }}"),
                "vars and coeffs sizes do not match in objective",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} objective {{ vars: 9 coeffs: 1 }}"),
                "Out of bound integer variable 9 in objective",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} objective {{ vars: 0 coeffs: 9223372036854775807 }}"),
                "Possible integer overflow in objective",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} objective {{ offset: inf }}"),
                "Objective offset must be finite",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} objective {{ scaling_factor: nan }}"),
                "nan, inf or subnormal scaling factor",
            ),
            (
                false,
                format!("{HALF_RANGE_VARIABLE} objective {{ vars: 0 coeffs: 1 scaling_factor: 1e300 }}"),
                "Possible floating point overflow in objective when multiplied by the scaling factor",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} objective {{}} floating_point_objective {{}}"),
                "A model cannot have both an objective and a floating point objective",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} objective {{ integer_scaling_factor: 2 }}"),
                "Objective integer scaling or offset is set without an objective domain",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} objective {{ domain: [1, 2] integer_scaling_factor: 9223372036854775807 }}"),
                "Internal fields related to the postsolve of the integer objective",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} objective {{ domain: [9223372036854775806, 9223372036854775806] integer_before_offset: 1 }}"),
                "Internal fields related to the postsolve of the integer objective",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} objective {{ domain: [1, 2] integer_scaling_factor: 2 integer_after_offset: 9223372036854775806 }}"),
                "Internal fields related to the postsolve of the integer objective",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} search_strategy {{ variable_selection_strategy: 9 }}"),
                "Unknown or unsupported variable_selection_strategy: 9",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} search_strategy {{ domain_reduction_strategy: 9 }}"),
                "Unknown or unsupported domain_reduction_strategy: 9",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} search_strategy {{ variables: 0 exprs {{}} }}"),
                "Strategy can't have both variables and exprs",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} search_strategy {{ variables: 9 }}"),
                "Invalid variable reference in strategy",
            ),
            (
                false,
                "variables { domain: [0, 200000] } search_strategy { variables: -1 domain_reduction_strategy: SELECT_MEDIAN_VALUE }".into(),
                "Variable #0 has a domain too large to be used in a SELECT_MEDIAN_VALUE value selection strategy",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} search_strategy {{ exprs {{ vars: [0, 1] coeffs: [1, 1] }} }}"),
                "expression must be affine in strategy",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} search_strategy {{ exprs {{ vars: 9 coeffs: 1 }} }}"),
                "Invalid variable reference in strategy",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} search_strategy {{ exprs {{ vars: -1 coeffs: 1 }} }}"),
                "Invalid affine expr in strategy",
            ),
            (
                false,
                "variables { domain: [0, 200000] } search_strategy { exprs { vars: 0 coeffs: 1 } domain_reduction_strategy: SELECT_MEDIAN_VALUE }".into(),
                "Variable #0 has a domain too large to be used in a SELECT_MEDIAN_VALUE value selection strategy",
            ),
            (
                false,
                format!("{FOUR_VARIABLES} solution_hint {{ vars: 0 values: 9223372036854775807 }}"),
                "The solution hint cannot contain the INT_MIN or INT_MAX values",
            ),
        ];

        for (index, (after_presolve, text, expected)) in cases.iter().enumerate() {
            // Act
            let model = model_of(text);
            let error = validate_cp_model(&model, *after_presolve);

            // Assert
            assert!(
                error.contains(expected),
                "case {index}: {error:?} does not contain {expected:?}"
            );
        }
    }

    #[test]
    fn validate_cp_model_accepts_each_kind_of_well_formed_constraint() {
        // Arrange
        let cases: Vec<String> = vec![
            format!("{FOUR_VARIABLES} constraints {{ bool_or {{ literals: [2, -4] }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ bool_and {{ literals: [2, -4] }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ at_most_one {{ literals: [2, 3] }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ exactly_one {{ literals: [2, 3] }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ bool_xor {{ literals: [2, 3] }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ linear {{ vars: [0, 1] coeffs: [1, 2] domain: [0, 5] }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ lin_max {{ target {{ vars: 0 coeffs: 1 }} exprs {{ vars: 1 coeffs: 1 }} }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ int_prod {{ target {{ vars: 0 coeffs: 1 }} exprs {{ vars: 1 coeffs: 1 }} exprs {{}} }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ int_div {{ target {{ vars: 0 coeffs: 1 }} exprs {{ vars: 1 coeffs: 1 }} exprs {{ offset: 2 }} }} }}"),
            "variables { domain: [1, 2] } variables { domain: [1, 2] } variables { domain: [1, 2] } variables { domain: [0, 5] } \
             constraints { int_div { target { vars: 3 coeffs: 1 } exprs {} exprs { vars: [0, 1, 2] coeffs: [1, 1, 1] } } }"
                .into(),
            format!("{FOUR_VARIABLES} constraints {{ int_mod {{ target {{ vars: 0 coeffs: 1 }} exprs {{ vars: 1 coeffs: 1 }} exprs {{ offset: 3 }} }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ inverse {{ f_direct: [0, 1] f_inverse: [1, 0] }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ all_diff {{ exprs {{ vars: 0 coeffs: 1 }} exprs {{ vars: 1 coeffs: 1 }} }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ element {{ index: 0 target: 1 vars: [2, 3] }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ element {{ linear_index {{ vars: 0 coeffs: 1 }} linear_target {{ vars: 1 coeffs: 1 }} exprs {{ vars: 2 coeffs: 1 }} }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ table {{ vars: [0, 1] values: [1, 2, 3, 4] }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ table {{ exprs {{ vars: 0 coeffs: 1 }} values: [1, 2] }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ table {{}} }}"),
            format!("{FOUR_VARIABLES} constraints {{ automaton {{ vars: 0 transition_tail: 0 transition_head: 1 transition_label: 0 final_states: 1 }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ circuit {{ tails: [0, 1] heads: [1, 0] literals: [2, 3] }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ routes {{ tails: [0, 1] heads: [1, 0] literals: [2, 3] }} }}"),
            format!("{FOUR_VARIABLES} {INTERVAL} constraints {{ no_overlap {{ intervals: 0 }} }}"),
            format!("{FOUR_VARIABLES} {INTERVAL} constraints {{ no_overlap_2d {{ x_intervals: 0 y_intervals: 0 }} }}"),
            format!("{FOUR_VARIABLES} {INTERVAL} constraints {{ cumulative {{ capacity {{ offset: 3 }} intervals: 0 demands {{ offset: 1 }} }} }}"),
            format!("{FOUR_VARIABLES} constraints {{ reservoir {{ time_exprs {{ vars: 0 coeffs: 1 }} level_changes {{ offset: 1 }} active_literals: 2 max_level: 3 }} }}"),
            format!("{FOUR_VARIABLES} objective {{ vars: [0, 1] coeffs: [1, 2] scaling_factor: 2 offset: 1 domain: [0, 30] integer_scaling_factor: 2 }}"),
            format!("{FOUR_VARIABLES} search_strategy {{ variables: [0, -2] variable_selection_strategy: CHOOSE_LOWEST_MIN domain_reduction_strategy: SELECT_MEDIAN_VALUE }}"),
            format!("{FOUR_VARIABLES} search_strategy {{ exprs {{ vars: 0 coeffs: 1 }} domain_reduction_strategy: SELECT_MEDIAN_VALUE }}"),
            format!("{FOUR_VARIABLES} solution_hint {{ vars: [0, 1] values: [1, 2] }}"),
        ];

        for (index, text) in cases.iter().enumerate() {
            // Act
            let model = model_of(text);
            let error = validate_cp_model(&model, false);

            // Assert
            assert_eq!(error, "", "case {index}");
        }
    }

    #[test]
    fn validate_input_cp_model_checks_the_floating_point_objective() {
        // Arrange
        // Each row is: the largest valid magnitude, the model text, a part of the error.
        let cases: Vec<(Option<f64>, String, &str)> = vec![
            (
                None,
                format!(
                    "{FOUR_VARIABLES} floating_point_objective {{ vars: [0, 1] coeffs: [1.5, -2] offset: 3 }}"
                ),
                "",
            ),
            (
                None,
                format!("{FOUR_VARIABLES} floating_point_objective {{ vars: 0 }}"),
                "vars and coeffs sizes do not match in objective",
            ),
            (
                None,
                format!("{FOUR_VARIABLES} floating_point_objective {{ vars: 9 coeffs: 1 }}"),
                "Out of bound integer variable 9 in objective",
            ),
            (
                None,
                format!("{FOUR_VARIABLES} floating_point_objective {{ vars: 0 coeffs: inf }}"),
                "Coefficients must be finite in objective",
            ),
            (
                None,
                format!("{FOUR_VARIABLES} floating_point_objective {{ vars: 0 coeffs: 1e26 }}"),
                "Coefficients larger than params.mip_max_valid_magnitude() [value = 1e+25] in objective",
            ),
            (
                Some(1e300),
                format!(
                    "{HALF_RANGE_VARIABLE} floating_point_objective {{ vars: 0 coeffs: -1e300 }}"
                ),
                "Possible floating point overflow in objective",
            ),
            (
                None,
                format!("{FOUR_VARIABLES} floating_point_objective {{ offset: inf }}"),
                "Offset must be finite in objective",
            ),
            (
                None,
                format!(
                    "{FOUR_VARIABLES} variables {{ domain: [5, 1] }} floating_point_objective {{}}"
                ),
                "has an invalid domain() format",
            ),
        ];

        for (index, (magnitude, text, expected)) in cases.iter().enumerate() {
            let model = model_of(text);
            let params = SatParameters {
                mip_max_valid_magnitude: *magnitude,
                ..Default::default()
            };

            // Act
            let error = validate_input_cp_model(&params, &model);

            // Assert
            if expected.is_empty() {
                assert_eq!(error, "", "case {index}");
            } else {
                assert!(
                    error.contains(expected),
                    "case {index}: {error:?} does not contain {expected:?}"
                );
            }
        }
    }

    #[test]
    fn possible_integer_overflow_sets_the_implied_domain_and_rejects_large_terms() {
        // Arrange
        let model = model_of("variables { domain: [-2, 3] } variables { domain: [0, 5] }");
        let mut implied_domain = (0, 0);

        // Act and Assert
        assert!(!possible_integer_overflow(
            &model,
            &[0, 1],
            &[2, -1],
            4,
            Some(&mut implied_domain)
        ));
        assert_eq!(implied_domain, (-4 - 4 - 5, 4 + 6));
        assert!(possible_integer_overflow(
            &model,
            &[0],
            &[1],
            i64::MAX,
            None
        ));
        assert!(possible_integer_overflow(
            &model,
            &[0],
            &[1],
            i64::MIN,
            None
        ));
        assert!(possible_integer_overflow(
            &model,
            &[0],
            &[i64::MIN],
            0,
            None
        ));
        assert!(possible_integer_overflow(
            &model,
            &[0],
            &[i64::MAX / 2],
            0,
            None
        ));
        assert!(!possible_integer_overflow(&model, &[], &[], 0, None));
    }

    #[test]
    fn possible_integer_overflow_dies_on_a_negated_reference() {
        // Arrange
        let model = model_of("variables { domain: [0, 5] }");

        // Act and Assert
        expect_death(
            || {
                possible_integer_overflow(&model, &[-1], &[1], 0, None);
            },
            "assertion failed",
        );
    }

    #[test]
    fn linear_overflow_checker_sums_the_extreme_terms_and_stops_at_half_the_range() {
        // Arrange
        let mut checker = LinearOverflowChecker::default();

        // Act and Assert
        assert!(checker.add_term(3, -2, 4));
        assert_eq!((checker.sum_min, checker.sum_max), (-6, 12));
        assert!(checker.add_term(-1, 1, 5));
        assert_eq!((checker.sum_min, checker.sum_max), (-11, 12));
        assert!(!checker.add_term(i64::MIN, 0, 1));
        assert!(!checker.add_term(i64::MAX, 0, 1));
        assert!(!LinearOverflowChecker::default().add_term(1, 0, i64::MAX / 2 + 1));
    }

    #[test]
    fn six_digits_to_string_prints_like_percent_g() {
        // Arrange
        let cases: [(f64, &str); 14] = [
            (0.0, "0"),
            (-0.0, "-0"),
            (1.0, "1"),
            (-2.0, "-2"),
            (1.5, "1.5"),
            (123456.7, "123457"),
            (100000.0, "100000"),
            (1e6, "1e+06"),
            (999999.5, "1e+06"),
            (0.0001, "0.0001"),
            (0.00001, "1e-05"),
            (1e25, "1e+25"),
            (f64::INFINITY, "inf"),
            (f64::NAN, "nan"),
        ];

        for (value, expected) in cases {
            // Act
            let text = six_digits_to_string(value);

            // Assert
            assert_eq!(text, expected, "value {value:?}");
        }
        assert_eq!(six_digits_to_string(f64::NEG_INFINITY), "-inf");
    }
}
