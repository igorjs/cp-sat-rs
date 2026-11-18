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
// Ported from ortools/sat/primary_variables.h at commit c931f5f7. Changed: translated from C++ to Rust.
// Ported from ortools/sat/primary_variables.cc at commit c931f5f7. Changed: translated from C++ to Rust.

// Signed overflow is undefined upstream. Overflow checks are on here, so an arithmetic
// overflow in this module panics.

use std::collections::{BTreeMap, BTreeSet, VecDeque};

use cpsat_proto::constraint_proto::Constraint;
use cpsat_proto::{
    ConstraintProto, CpModelProto, LinearArgumentProto, LinearConstraintProto,
    LinearExpressionProto,
};
use cpsat_util::bitset::Bitset64;
use cpsat_util::sorted_interval_list::Domain;

use crate::cp_model_utils::{
    positive_ref, read_domain_from_proto, ref_is_positive, used_variables,
};

// This struct defines a way of splitting the variables of the model into two
// groups: primary variables and secondary variables. Those are specified so
// that the value of secondary_variables[i] is uniquely fixed by applying the
// constraint dependency_resolution_constraint_index[i] to the values of the
// primary variables and the values of the variables in the set
// {secondary_variables[0], ..., secondary_variables[i-1]}.
//
// The set of primary variables is implicitly defined by the set of variables
// that are not in secondary_variables.
//
// A useful property of this structure is that given an assignment of primary
// variables that corresponds to a feasible solution, we can deduce all the
// values of the secondary variables. Note that if the values of the primary
// variables are infeasible, then it might not be possible to deduce the values
// of the secondary variables.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct VariableRelationships {
    pub secondary_variables: Vec<i32>,
    pub dependency_resolution_constraint: Vec<ConstraintProto>,

    // A pair (x, y) means that one needs to compute the value of y before
    // computing the value of x. This defines an implicit dependency DAG for
    // computing the secondary variables from the primary.
    pub variable_dependencies: Vec<(i32, i32)>,

    // The list of model constraints that are redundant (i.e., satisfied by
    // construction) when the secondary variables are computed from the primary
    // ones. In other words, a model has a solution for a set of primary
    // variables {x_i} if and only if all the variable bounds and non-redundant
    // constraints are satisfied after the secondary variables have been computed
    // from the primary ones.
    pub redundant_constraint_indices: Vec<i32>,
}

// Declares that if all variables in input_vars and all but one of the
// variables in deducible_vars are known, then we can deduce the missing one.
// No variable should appear twice in the inputs.
//
// For example, if we have:
//  u + z + x = lin_max(x + 3, y)
//
// This function will assign:
//  deducible_vars = {u, z}
//  input_vars = {x, y}
//
// This declares that, for example, if x, y and u are all known, then z is
// known. On the other hand, if everything is known except x, then we can't
// deduce anything, since for some values of u, z and y, the constraint can be
// simplified to x + 3 = x + 3.
fn get_relationship_for_constraint(
    ct: &ConstraintProto,
    deducible_vars: &mut BTreeSet<i32>,
    input_vars: &mut BTreeSet<i32>,
    preferred_to_deduce: &mut i32,
) {
    deducible_vars.clear();
    input_vars.clear();
    *preferred_to_deduce = -1;
    if !ct.enforcement_literal.is_empty() {
        return;
    }
    match &ct.constraint {
        Some(Constraint::Linear(linear)) => {
            if read_domain_from_proto(linear).size() != 1 {
                return;
            }
            for i in 0..linear.vars.len() {
                if linear.coeffs[i] == 0 {
                    continue;
                }
                deducible_vars.insert(linear.vars[i]);
            }
        }
        Some(Constraint::LinMax(lin_max)) => {
            let default_target = LinearExpressionProto::default();
            let target = lin_max.target.as_ref().unwrap_or(&default_target);
            // We can deduce only the variables that are only in the target.
            for i in 0..target.vars.len() {
                if target.coeffs[i] == 0 {
                    continue;
                }
                deducible_vars.insert(target.vars[i]);
            }
            for expr in &lin_max.exprs {
                for &var in &expr.vars {
                    input_vars.insert(var);
                }
            }
            for var in input_vars.iter() {
                deducible_vars.remove(var);
            }
        }
        Some(Constraint::IntProd(int_prod)) => {
            let default_target = LinearExpressionProto::default();
            let target = int_prod.target.as_ref().unwrap_or(&default_target);
            let mut variable_appearance_count: BTreeMap<i32, i32> = BTreeMap::new();
            for &var in &target.vars {
                *variable_appearance_count.entry(var).or_default() += 1;
            }
            for expr in &int_prod.exprs {
                for &var in &expr.vars {
                    *variable_appearance_count.entry(var).or_default() += 1;
                }
            }
            for i in 0..target.vars.len() {
                if target.coeffs[i] == 0 {
                    continue;
                }
                let var = target.vars[i];
                if variable_appearance_count[&var] == 1 {
                    deducible_vars.insert(var);
                }
            }
            for expr in &int_prod.exprs {
                for i in 0..expr.vars.len() {
                    let var = expr.vars[i];
                    if variable_appearance_count[&var] == 1 {
                        // We might be tempted to make the variable deducible if the
                        // coefficient is 1, but we risk trying to deduce x from 0 = 0 * x.
                        // TODO(user): do it when the target domain doesn't include 0,
                        // but use preferred_to_deduce to prefer the target.
                        input_vars.insert(var);
                    }
                }
            }
            for (&var, &count) in &variable_appearance_count {
                if count != 1 {
                    input_vars.insert(var);
                }
            }
        }
        Some(Constraint::ExactlyOne(exactly_one)) => {
            for &lit in &exactly_one.literals {
                deducible_vars.insert(positive_ref(lit));
            }
        }
        _ => {}
    }
}

fn create_lin_max_from_linears_and_objective(
    model: &CpModelProto,
    var_for_target: i32,
    linear_constraint_indexes: &[i32],
    var_in_objective_is_negative: bool,
    new_constraint: &mut ConstraintProto,
) {
    // A variable that is in the objective with a positive coefficient and only
    // appears in inequalities will be at the lowest value that is greater than or
    // equal to the variable domain lower bound and that does not violate any
    // bound coming from the inequalities. A similar reasoning applies for a
    // variable with a negative coefficient in the objective.
    let mut lin_max = LinearArgumentProto::default();
    let target = lin_max.target.get_or_insert_default();
    target
        .coeffs
        .push(if var_in_objective_is_negative { -1 } else { 1 });
    target.vars.push(var_for_target);
    let var_domain: Domain = read_domain_from_proto(&model.variables[var_for_target as usize]);
    // Add the bound coming from the variable domain.
    if var_in_objective_is_negative {
        lin_max.exprs.push(LinearExpressionProto {
            offset: -var_domain.max(),
            ..Default::default()
        });
    } else {
        lin_max.exprs.push(LinearExpressionProto {
            offset: var_domain.min(),
            ..Default::default()
        });
    }

    for &c in linear_constraint_indexes {
        let Some(Constraint::Linear(lin)) = &model.constraints[c as usize].constraint else {
            panic!("constraint {c} is not a linear constraint");
        };
        let lin: &LinearConstraintProto = lin;
        let mut coeff: i64 = 0;
        for i in 0..lin.coeffs.len() {
            if lin.vars[i] == var_for_target {
                coeff = lin.coeffs[i];
                break;
            }
        }
        let mut expr = LinearExpressionProto::default();
        debug_assert_eq!(lin.domain.len(), 2);
        let expr_min = lin.domain[0];
        let expr_max = lin.domain[1];
        if (coeff < 0) == var_in_objective_is_negative {
            expr.offset = expr_min;
        } else {
            expr.offset = -expr_max;
        }
        let multiplier: i64 = if (coeff < 0) == var_in_objective_is_negative {
            -1
        } else {
            1
        };
        for i in 0..lin.coeffs.len() {
            if lin.vars[i] == var_for_target {
                continue;
            }
            expr.vars.push(lin.vars[i]);
            expr.coeffs.push(multiplier * lin.coeffs[i]);
        }
        lin_max.exprs.push(expr);
    }
    new_constraint.constraint = Some(Constraint::LinMax(lin_max));
}

fn is_objective_constraining(model: &CpModelProto) -> bool {
    let Some(objective) = &model.objective else {
        return false;
    };
    if objective.domain.is_empty() {
        return false;
    }
    if objective.domain.len() > 2 {
        return true;
    }
    let mut lb: i64 = 0;
    let mut ub: i64 = 0;
    for i in 0..objective.vars.len() {
        let var = objective.vars[i];
        let variable = &model.variables[var as usize];
        lb += objective.coeffs[i] * variable.domain[0];
        ub += objective.coeffs[i] * variable.domain[variable.domain.len() - 1];
    }
    if objective.domain[0] > lb || objective.domain[1] < ub {
        return true;
    }
    false
}

#[derive(Default)]
struct ConstraintData {
    // These sets will hold only the "undecided" variables. When a variable
    // is marked as primary or secondary, it will be removed.
    deducible_vars: BTreeSet<i32>,
    input_vars: BTreeSet<i32>,

    preferred_to_deduce: i32,

    // If a variable participates in the model only via linear inequalities and
    // the objective, and *all* the other variables in those inequalities are
    // already tagged as primary or secondary, then this variable can be flagged
    // as a secondary variable and can be computed as a lin_max of the others.
    is_linear_inequality: bool,
}

// The C++ code makes this a lambda inside the main loop. A Rust closure would hold
// `vars_queue` and `constraints_to_check` mutably for the rest of the loop body, so the
// state it changes is passed in.
fn update_constraints_after_var_is_decided(
    v: i32,
    constraints_per_var: &[Vec<i32>],
    constraint_data: &mut [ConstraintData],
    count_of_unresolved_linear_inequalities_per_var: &mut [i32],
    vars_queue: &mut VecDeque<i32>,
    constraints_to_check: &mut Vec<i32>,
) {
    for &c in &constraints_per_var[v as usize] {
        let data = &mut constraint_data[c as usize];
        data.deducible_vars.remove(&v);
        data.input_vars.remove(&v);
        if data.input_vars.len() + data.deducible_vars.len() != 1 {
            // Two of the variables for this constraint are neither primary nor
            // secondary yet, we cannot deduce the value of anything using this
            // constraint.
            continue;
        }
        if data.is_linear_inequality && data.deducible_vars.len() == 1 {
            let deducible_var = *data.deducible_vars.first().expect("one deducible variable");
            count_of_unresolved_linear_inequalities_per_var[deducible_var as usize] -= 1;
            if count_of_unresolved_linear_inequalities_per_var[deducible_var as usize] == 0 {
                // Now we can deduce a new variable from linears, process it ASAP!
                vars_queue.push_front(deducible_var);
            }
        } else if data.deducible_vars.is_empty() {
            // Same as the test for the case `z = lin_max(x, y)` above.
            data.input_vars.clear();
        } else {
            // This constraint fixes a secondary variable, enqueue it!
            constraints_to_check.push(c);
        }
    }
}

// Computes the variable relationships for a given model. Note that there are
// multiple possible ways variables can be split into primary and secondary, so
// this function will use a heuristic to try to find as many secondary
// variables as possible. This runs in linear time in the model size (i.e., the
// sum of the number of variables over the constraints).
pub fn compute_variable_relationships(model: &CpModelProto) -> VariableRelationships {
    let mut result = VariableRelationships::default();
    let num_variables = model.variables.len();
    let mut var_is_secondary: Bitset64<i32> = Bitset64::new(num_variables as i32);
    let mut var_is_primary: Bitset64<i32> = Bitset64::new(num_variables as i32);
    let mut num_times_variable_appears_as_input = vec![0i32; num_variables];
    let mut num_times_variable_appears_as_deducible = vec![0i32; num_variables];
    let mut num_times_variable_appears_as_preferred_to_deduce = vec![0i32; num_variables];

    let mut constraint_data: Vec<ConstraintData> = (0..model.constraints.len())
        .map(|_| ConstraintData::default())
        .collect();
    let mut constraints_per_var: Vec<Vec<i32>> = vec![Vec::new(); num_variables];

    let mut var_appears_only_in_objective_and_linear: Bitset64<i32> =
        Bitset64::new(num_variables as i32);
    let mut var_in_objective_is_negative: Bitset64<i32> = Bitset64::new(num_variables as i32);
    if !is_objective_constraining(model) {
        // TODO(user): if we have a constraining objective we can suppose a
        // non-constraining one + a linear constraint. But this will allow us to
        // find at most one new secondary variable, since all the variables in the
        // objective will be connected via this linear constraint.
        if let Some(objective) = &model.objective {
            for i in 0..objective.vars.len() {
                if objective.coeffs[i] == 0 {
                    continue;
                }
                var_appears_only_in_objective_and_linear.set(objective.vars[i]);
                if objective.coeffs[i] < 0 {
                    var_in_objective_is_negative.set(objective.vars[i]);
                }
            }
        }
    }
    for (data, ct) in constraint_data.iter_mut().zip(&model.constraints) {
        data.is_linear_inequality = false;
        get_relationship_for_constraint(
            ct,
            &mut data.deducible_vars,
            &mut data.input_vars,
            &mut data.preferred_to_deduce,
        );
        // Now prepare the data for handling the case of variables that only
        // appear in the objective and linear inequalities. There are two cases:
        // - either the constraint is one such linear inequality and we flag it
        //   as such;
        // - if not, we flag all the variables used in this constraint as appearing
        //   in constraints that are not linear inequalities.
        let linear = match &ct.constraint {
            Some(Constraint::Linear(linear)) => Some(linear),
            _ => None,
        };
        if let Some(linear) = linear
            && data.deducible_vars.is_empty() // Not allowing fixing a secondary var
                                              // directly (i.e., an equality)
            && ct.enforcement_literal.is_empty()
            && linear.domain.len() == 2
        {
            // This is the kind of inequality we might use for a lin_max
            data.is_linear_inequality = true;
            for i in 0..linear.vars.len() {
                let var = linear.vars[i];
                if !var_appears_only_in_objective_and_linear.is_set(var) {
                    data.input_vars.insert(var);
                    continue;
                }
                if linear.coeffs[i] == 0 {
                    continue;
                }
                // wrapping_abs gives the value an optimised build gives for kint64min.
                if linear.coeffs[i].wrapping_abs() == 1 {
                    data.deducible_vars.insert(var);
                } else {
                    data.input_vars.insert(var);
                    // TODO(user): we can support non-unit coefficients to deduce a
                    // lin_max from the objective. It will become more difficult, since
                    // first we will need to compute the lcm of all coefficients (and
                    // avoid overflow). Then, we will need to build a constraint that will
                    // be div(target, lin_max(exprs) + lcm - 1, lcm).
                    var_appears_only_in_objective_and_linear.set_with_value(var, false);
                }
            }
        } else {
            // For other kinds of constraints, tag those variables as "used
            // elsewhere".
            for var in used_variables(ct) {
                var_appears_only_in_objective_and_linear.set_with_value(var, false);
            }
        }
    }

    // In the loop above, we lazily set some variables as deducible from linear
    // inequalities if they only appeared in the objective and linear inequalities
    // when we saw the constraint, but we did not check how they were used in
    // following constraints. Now remove them if they were used in other
    // constraints.
    let mut deducible_vars_to_remove: Vec<i32> = Vec::new();
    for data in &mut constraint_data {
        deducible_vars_to_remove.clear();
        if !data.is_linear_inequality {
            continue;
        }
        for &var in &data.deducible_vars {
            if !var_appears_only_in_objective_and_linear.is_set(var) {
                deducible_vars_to_remove.push(var);
                data.input_vars.insert(var);
            }
        }
        for var in &deducible_vars_to_remove {
            data.deducible_vars.remove(var);
        }
        if data.deducible_vars.is_empty() {
            data.is_linear_inequality = false;
        }
    }

    for (c, data) in constraint_data.iter_mut().enumerate() {
        if data.deducible_vars.is_empty() {
            data.input_vars.clear();
            continue;
        }
        if data.preferred_to_deduce != -1 {
            num_times_variable_appears_as_preferred_to_deduce[data.preferred_to_deduce as usize] +=
                1;
        }
        for &v in &data.deducible_vars {
            constraints_per_var[v as usize].push(c as i32);
            num_times_variable_appears_as_deducible[v as usize] += 1;
        }
        for &v in &data.input_vars {
            constraints_per_var[v as usize].push(c as i32);
            num_times_variable_appears_as_input[v as usize] += 1;
        }
    }

    // Variables that cannot be potentially deduced using any constraints are
    // primary. Flag them as such and strip them from our data structures.
    for v in 0..num_variables {
        if num_times_variable_appears_as_deducible[v] != 0 {
            continue;
        }
        num_times_variable_appears_as_input[v] = 0;
        var_is_primary.set(v as i32);
        for &c in &constraints_per_var[v] {
            let data = &mut constraint_data[c as usize];
            data.deducible_vars.remove(&(v as i32));
            data.input_vars.remove(&(v as i32));
        }
        constraints_per_var[v].clear();
    }

    // Now, for variables that only appear in the objective and linear
    // inequalities, we count how far we are from being able to deduce their
    // value. In practice, we count the number of linear inequalities in which
    // this variable appears alongside another variable that we have not decided
    // to be primary or secondary yet. When this count reaches 0, it means we can
    // create a lin_max constraint to deduce its value.
    let mut count_of_unresolved_linear_inequalities_per_var = vec![0i32; num_variables];
    for data in &constraint_data {
        if !data.is_linear_inequality {
            continue;
        }
        if data.input_vars.len() + data.deducible_vars.len() > 1 {
            for &v in &data.input_vars {
                count_of_unresolved_linear_inequalities_per_var[v as usize] += 1;
            }
            for &v in &data.deducible_vars {
                count_of_unresolved_linear_inequalities_per_var[v as usize] += 1;
            }
        }
    }
    // Now do a greedy heuristic: we will take each variable in a particular
    // order, and if the variable can be deduced from other variables that we have
    // already decided to declare as primary or secondary, we will mark it as
    // secondary. Otherwise we will mark it as primary. In any case, after we do
    // that, we will look at all the constraints that use this variable and see
    // if it allows deducing some variable. If yes, mark the variable that can be
    // deduced as secondary, look at the constraints that use it, and so on until
    // we reach a fixed point. The heuristic for the order is to try to process
    // first the variables that are more "useful" to be marked as primary, so it
    // allows us to mark more variables as secondary in the following.
    let mut vars_queue: VecDeque<i32> = VecDeque::new();
    for v in 0..num_variables as i32 {
        if var_is_primary.is_set(v) || var_is_secondary.is_set(v) {
            continue;
        }
        vars_queue.push_back(v);
    }
    // The sort is stable, as c_stable_sort is.
    vars_queue.make_contiguous().sort_by_key(|&a| {
        let a = a as usize;
        let a_diff_usage =
            num_times_variable_appears_as_deducible[a] - num_times_variable_appears_as_input[a];
        (
            a_diff_usage,
            -num_times_variable_appears_as_preferred_to_deduce[a],
            -num_times_variable_appears_as_deducible[a],
        )
    });

    // Put in front of the queue all the variables that can readily be deduced
    // using some constraint.
    for data in &constraint_data {
        if data.input_vars.len() + data.deducible_vars.len() != 1 {
            continue;
        }
        if !data.deducible_vars.is_empty() {
            vars_queue.push_front(*data.deducible_vars.first().expect("a deducible variable"));
        } else if data.is_linear_inequality {
            vars_queue.push_front(*data.input_vars.first().expect("an input variable"));
        }
    }

    let mut constraints_to_check: Vec<i32> = Vec::new();
    while let Some(v) = vars_queue.pop_front() {
        if var_is_secondary.is_set(v) || var_is_primary.is_set(v) {
            continue;
        }
        // First, we will decide whether we should mark `v` as secondary or primary
        // using the equality constraints.
        for &c in &constraints_per_var[v as usize] {
            let data = &mut constraint_data[c as usize];
            if data.is_linear_inequality {
                continue;
            }
            if data.deducible_vars.len() + data.input_vars.len() != 1 {
                // One of its inputs is neither primary nor secondary yet, we cannot
                // deduce the value of `v` using this constraint.
                continue;
            }

            // There is a single undecided variable for this constraint. Thus `v` is
            // either an input or a deducible variable. Let's check.
            if data.deducible_vars.is_empty() {
                // This is a strange case, like `z = lin_max(x, y)`, where `z` and `y`
                // are primary (we cannot deduce `x`). Let's just flag this constraint
                // as useless from now on.
                data.input_vars.clear();
                continue;
            }
            var_is_secondary.set(v);
            result.secondary_variables.push(v);
            result
                .dependency_resolution_constraint
                .push(model.constraints[c as usize].clone());
            result.redundant_constraint_indices.push(c);
            break;
        }

        // We couldn't deduce the value of `v` from any constraint, check if it only
        // appears in linear inequalities.
        if !var_is_secondary.is_set(v) {
            if var_appears_only_in_objective_and_linear.is_set(v)
                && count_of_unresolved_linear_inequalities_per_var[v as usize] == 0
            {
                var_is_secondary.set(v);
                result.secondary_variables.push(v);
                result
                    .dependency_resolution_constraint
                    .push(ConstraintProto::default());
                create_lin_max_from_linears_and_objective(
                    model,
                    v,
                    &constraints_per_var[v as usize],
                    var_in_objective_is_negative.is_set(v),
                    result
                        .dependency_resolution_constraint
                        .last_mut()
                        .expect("a constraint was just pushed"),
                );
                // TODO(user): set result.redundant_constraint_indices?
            } else {
                // We can't deduce the value of `v` from what we have so far, flag it as
                // primary.
                var_is_primary.set(v);
            }
        }

        // At this point we have decided to make `v` primary or secondary, so we
        // can remove it from the model and maybe lazily deduce some variables.

        // In any case, this variable is now decided, so we update the number of
        // undecided variables in all the constraints that use it.
        debug_assert!(constraints_to_check.is_empty());
        update_constraints_after_var_is_decided(
            v,
            &constraints_per_var,
            &mut constraint_data,
            &mut count_of_unresolved_linear_inequalities_per_var,
            &mut vars_queue,
            &mut constraints_to_check,
        );

        // Now, deduce everything that becomes trivially deducible until we reach a
        // fixed point.
        while let Some(c) = constraints_to_check.pop() {
            let data = &constraint_data[c as usize];
            debug_assert!(data.deducible_vars.len() <= 1);
            if data.deducible_vars.len() != 1 {
                continue;
            }
            let single_deducible_var =
                *data.deducible_vars.first().expect("one deducible variable");
            var_is_secondary.set(single_deducible_var);
            update_constraints_after_var_is_decided(
                single_deducible_var,
                &constraints_per_var,
                &mut constraint_data,
                &mut count_of_unresolved_linear_inequalities_per_var,
                &mut vars_queue,
                &mut constraints_to_check,
            );
            result.secondary_variables.push(single_deducible_var);
            result
                .dependency_resolution_constraint
                .push(model.constraints[c as usize].clone());
            result.redundant_constraint_indices.push(c);
        }
    }

    for i in 0..result.secondary_variables.len() {
        let var = result.secondary_variables[i];
        let mut data = ConstraintData::default();
        let ct = &result.dependency_resolution_constraint[i];
        get_relationship_for_constraint(
            ct,
            &mut data.deducible_vars,
            &mut data.input_vars,
            &mut data.preferred_to_deduce,
        );
        for &v in &data.input_vars {
            if var_is_secondary.is_set(v) {
                result.variable_dependencies.push((var, v));
            }
        }
        for &v in &data.deducible_vars {
            if var_is_secondary.is_set(v) && v != var {
                result.variable_dependencies.push((var, v));
            }
        }
    }
    result
}

// Given a vector with a partial solution where only the primary variables have
// a correct value, this function will overwrite the values of the secondary
// variables so that the solution is complete and valid.
pub fn compute_all_variables_from_primary_variables(
    model: &CpModelProto,
    relationships: &VariableRelationships,
    solution: &mut [i64],
) -> bool {
    let mut dependent_variables_set: Bitset64<i32> = Bitset64::new(model.variables.len() as i32);
    for &var in &relationships.secondary_variables {
        dependent_variables_set.set(var);
    }
    for i in 0..relationships.secondary_variables.len() {
        let var = relationships.secondary_variables[i];
        let ct = &relationships.dependency_resolution_constraint[i];
        match &ct.constraint {
            Some(Constraint::Linear(linear)) => {
                debug_assert_eq!(read_domain_from_proto(linear).size(), 1);
                let mut sum_of_free_variables: i64 = read_domain_from_proto(linear).min();
                let mut coeff_of_var: i64 = 1;
                for j in 0..linear.vars.len() {
                    if linear.vars[j] == var {
                        coeff_of_var = linear.coeffs[j];
                        continue;
                    }
                    debug_assert!(!dependent_variables_set.is_set(linear.vars[j]));
                    sum_of_free_variables -= linear.coeffs[j] * solution[linear.vars[j] as usize];
                }
                solution[var as usize] = sum_of_free_variables / coeff_of_var;
            }
            Some(Constraint::LinMax(lin_max)) => {
                let mut max: i64 = i64::MIN;
                for expr in &lin_max.exprs {
                    let mut expr_value: i64 = expr.offset;
                    for j in 0..expr.vars.len() {
                        debug_assert!(!dependent_variables_set.is_set(expr.vars[j]));
                        expr_value += expr.coeffs[j] * solution[expr.vars[j] as usize];
                    }
                    max = max.max(expr_value);
                }
                let default_target = LinearExpressionProto::default();
                let target = lin_max.target.as_ref().unwrap_or(&default_target);
                let mut coeff_of_var: i64 = 1;
                for j in 0..target.vars.len() {
                    if target.vars[j] == var {
                        coeff_of_var = target.coeffs[j];
                        continue;
                    }
                    debug_assert!(!dependent_variables_set.is_set(target.vars[j]));
                    max -= target.coeffs[j] * solution[target.vars[j] as usize];
                }
                max -= target.offset;
                solution[var as usize] = max / coeff_of_var;
            }
            Some(Constraint::IntProd(int_prod)) => {
                let mut product: i64 = 1;
                for expr in &int_prod.exprs {
                    let mut expr_value: i64 = expr.offset;
                    for j in 0..expr.vars.len() {
                        debug_assert!(!dependent_variables_set.is_set(expr.vars[j]));
                        expr_value += solution[expr.vars[j] as usize] * expr.coeffs[j];
                    }
                    product *= expr_value;
                }
                let mut coeff_of_var: i64 = 1;
                let default_target = LinearExpressionProto::default();
                let target = int_prod.target.as_ref().unwrap_or(&default_target);
                for j in 0..target.vars.len() {
                    if target.vars[j] == var {
                        coeff_of_var = target.coeffs[j];
                        continue;
                    }
                    debug_assert!(!dependent_variables_set.is_set(target.vars[j]));
                    product -= target.coeffs[j] * solution[target.vars[j] as usize];
                }
                product -= target.offset;
                solution[var as usize] = product / coeff_of_var;
            }
            Some(Constraint::ExactlyOne(exactly_one)) => {
                solution[var as usize] = 0;
                let mut sum: i32 = 0;
                for &lit in &exactly_one.literals {
                    let positive_ref = positive_ref(lit);
                    debug_assert!(
                        positive_ref == var || !dependent_variables_set.is_set(positive_ref)
                    );
                    let term = if ref_is_positive(lit) {
                        solution[positive_ref as usize]
                    } else {
                        1 - solution[positive_ref as usize]
                    };
                    // The C++ `sum += int64` converts the result back to int.
                    sum = (i64::from(sum) + term) as i32;
                }
                if sum != 1 {
                    solution[var as usize] ^= 1;
                }
            }
            _ => {}
        }
        dependent_variables_set.set_with_value(var, false);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpsat_testutil::parse_test_proto;

    #[test]
    fn primary_variables_test_basic_example() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ -10, 10 ] }
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 10 ] }
    variables { domain: [ 0, 10 ] }
    variables { domain: [ -100, 10 ] }
    constraints {
      linear {
        vars: [ 0, 1, 2, 4 ]
        coeffs: [ 1, 1, 1, 2 ]
        domain: [ 0, 0 ]
      }
    }
    constraints {
      lin_max {
        target {
          vars: [ 5 ]
          coeffs: [ 1 ]
        }
        exprs {
          vars: [ 1 ]
          coeffs: [ 1 ]
        }
        exprs {
          vars: [ 3 ]
          coeffs: [ 1 ]
        }
      }
    }
    constraints {
      linear {
        vars: [ 0, 1, 2 ]
        coeffs: [ 1, 1, 2 ]
        domain: [ 0, 0 ]
      }
    }
    constraints {
      linear {
        vars: [ 5, 6 ]
        coeffs: [ 1, 1 ]
        domain: [ 0, 0 ]
      }
    }
  "#,
        );
        let solution: Vec<i64> = vec![-1, 1, 0, 4, 0, 4, -4];

        // Act
        let relationships = compute_variable_relationships(&model);
        let mut all_variables = solution.clone();
        for &var in &relationships.secondary_variables {
            all_variables[var as usize] = -999999;
        }
        let computed = compute_all_variables_from_primary_variables(
            &model,
            &relationships,
            &mut all_variables,
        );

        // Assert
        assert!(relationships.secondary_variables.len() > 1);
        assert!(relationships.variable_dependencies.contains(&(6, 5)));
        assert!(computed);
        assert_eq!(all_variables, solution);
    }

    #[test]
    fn primary_variables_test_with_int_prod() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ -10, 10 ] }
    variables { domain: [ -10, 10 ] }
    constraints {
      int_prod {
        target { vars: 0 coeffs: 1 offset: -6 }
        exprs { vars: 1 coeffs: 3 offset: 1 }
      }
    }
  "#,
        );
        let solution: Vec<i64> = vec![10, 1];

        // Act
        let relationships = compute_variable_relationships(&model);
        let mut all_variables = solution.clone();
        for &var in &relationships.secondary_variables {
            all_variables[var as usize] = -999999;
        }
        let computed = compute_all_variables_from_primary_variables(
            &model,
            &relationships,
            &mut all_variables,
        );

        // Assert
        assert!(computed);
        assert_eq!(all_variables, solution);
    }

    #[test]
    fn primary_variables_test_with_exactly_one() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    variables { domain: [ 0, 1 ] }
    constraints { exactly_one { literals: [ 0, 1, 2, 3 ] } }
  "#,
        );
        let expected: ConstraintProto = parse_test_proto(
            r#"
    exactly_one { literals: [ 0, 1, 2, 3 ] }
  "#,
        );

        // Act
        let relationships = compute_variable_relationships(&model);

        // Assert
        assert_eq!(relationships.secondary_variables.len(), 1);
        assert_eq!(
            relationships.dependency_resolution_constraint,
            vec![expected]
        );
    }
}
