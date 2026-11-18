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
// Ported from ortools/sat/cp_model_utils.h at commit c931f5f7. Changed: translated from C++ to Rust.
// Ported from ortools/sat/cp_model_utils.cc at commit c931f5f7. Changed: translated from C++ to Rust.

use std::cmp::Ordering;
use std::collections::{HashMap, HashSet};
use std::fmt;
use std::fs;
use std::sync::atomic::AtomicBool;
use std::sync::{LazyLock, Mutex};

use cpsat_base::hash::fasthash64;
use cpsat_proto::constraint_proto::Constraint;
use cpsat_proto::{
    ConstraintProto, CpModelProto, CpObjectiveProto, FloatObjectiveProto, IntegerVariableProto,
    LinearArgumentProto, LinearConstraintProto, LinearExpressionProto,
};
use cpsat_util::bitset::Bitset64;
use cpsat_util::full_precision_inequalities::TightSum;
use cpsat_util::saturated_arithmetic::{at_min_or_max_int64, cap_prod};
use cpsat_util::sorted_interval_list::Domain;

use crate::sat_literal::{BooleanVariable, Literal};

// DEBUG ONLY. When set to true, SolveCpModel() will dump its model protos
// (original model, presolved model, mapping model) in text format to
// 'FLAGS_cp_model_dump_prefix'{model|presolved_model|mapping_model}.pb.txt.
pub static CP_MODEL_DUMP_MODELS: AtomicBool = AtomicBool::new(false);

// Prefix filename for all dumped files.
pub static CP_MODEL_DUMP_PREFIX: LazyLock<Mutex<String>> = LazyLock::new(|| {
    Mutex::new(
        if cfg!(target_env = "msvc") {
            ".\\"
        } else {
            "/tmp/"
        }
        .to_owned(),
    )
});

// DEBUG ONLY. When set to true, solve will dump all lns or objective_shaving
// submodel protos in text format to 'FLAGS_cp_model_dump_prefix'xxx.pb.txt.
pub static CP_MODEL_DUMP_SUBMODELS: AtomicBool = AtomicBool::new(false);

// Small utility functions to deal with negative variable/literal references.
pub fn negated_ref(r#ref: i32) -> i32 {
    -r#ref - 1
}
pub fn positive_ref(r#ref: i32) -> i32 {
    r#ref.max(negated_ref(r#ref))
}
pub fn ref_is_positive(r#ref: i32) -> bool {
    r#ref >= 0
}

// If `lits` (sorted in ascending signed-int order) contains both `~x` and `x`
// for some variable `x`, returns `x >= 0`; otherwise returns -1.
pub fn find_complementary_var_in_sorted_literals(lits: &[i32]) -> i32 {
    debug_assert!(lits.is_sorted());
    // Since `lits` is sorted ascending, negated literals (`-var - 1 < 0`) at the
    // front and positive literals (`var >= 0`) at the back both appear in
    // descending `var` order as `left` advances and `right` retreats.
    let mut left: i32 = 0;
    let mut right: i32 = lits.len() as i32 - 1;
    while left < right
        && !ref_is_positive(lits[left as usize])
        && ref_is_positive(lits[right as usize])
    {
        let neg_var = positive_ref(lits[left as usize]);
        let pos_var = lits[right as usize];
        if neg_var == pos_var {
            return pos_var;
        }
        if neg_var > pos_var {
            left += 1;
        } else {
            right -= 1;
        }
    }
    -1
}

pub fn sorted_literals_have_opposite_pair(lits: &[i32]) -> bool {
    find_complementary_var_in_sorted_literals(lits) != -1
}

// Returns true iff sorted literal lists `a` and `b` contain complementary
// literals (`lit` in `a` and `NegatedRef(lit)` in `b`).
pub fn sorted_literals_have_opposite_across(a: &[i32], b: &[i32]) -> bool {
    debug_assert!(a.is_sorted());
    debug_assert!(b.is_sorted());
    // Since `b` is sorted ascending, `NegatedRef(b[j])` is sorted ascending as
    // `j` decreases.
    let mut i: usize = 0;
    let mut j: i32 = b.len() as i32 - 1;
    while i < a.len() && j >= 0 {
        let neg_b = negated_ref(b[j as usize]);
        if a[i] == neg_b {
            return true;
        }
        if a[i] < neg_b {
            i += 1;
        } else {
            j -= 1;
        }
    }
    false
}

// Small utility functions to deal with half-reified constraints.
pub fn has_enforcement_literal(ct: &ConstraintProto) -> bool {
    !ct.enforcement_literal.is_empty()
}
pub fn enforcement_literal(ct: &ConstraintProto) -> i32 {
    ct.enforcement_literal[0]
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct AffineExpr {
    // The variable in the CpModelProto (-1 if constant).
    pub var: i32,
    // Coefficient of the variable.
    pub coeff: i64,
    // Constant offset.
    pub offset: i64,
}

// The default member values of the C++ struct.
impl Default for AffineExpr {
    fn default() -> Self {
        AffineExpr {
            var: -1,
            coeff: 0,
            offset: 0,
        }
    }
}

impl AffineExpr {
    // `Const` is a Rust keyword once lowered, so the name takes a trailing underscore.
    pub const fn const_(cst: i64) -> AffineExpr {
        AffineExpr {
            var: -1,
            coeff: 0,
            offset: cst,
        }
    }

    pub const fn var(var: i32) -> AffineExpr {
        AffineExpr {
            var,
            coeff: 1,
            offset: 0,
        }
    }

    pub const fn is_constant(&self) -> bool {
        self.var < 0 || self.coeff == 0
    }

    pub fn constant_value(&self) -> i64 {
        debug_assert!(self.is_constant());
        self.offset
    }
}

// `ToString()` and `AbslStringify` of the C++ struct.
impl fmt::Display for AffineExpr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.var < 0 || self.coeff == 0 {
            return write!(f, "{}", self.offset);
        }
        // `-offset` at sat/cp_model_utils.cc:1558 gives the smallest int64 back for the smallest
        // int64: the reference build prints "X0 - -9223372036854775808" for it.
        let offset_str = if self.offset > 0 {
            format!(" + {}", self.offset)
        } else if self.offset < 0 {
            format!(" - {}", self.offset.wrapping_neg())
        } else {
            String::new()
        };
        if self.coeff == 1 {
            return write!(f, "X{}{}", self.var, offset_str);
        }
        if self.coeff == -1 {
            return write!(f, "-X{}{}", self.var, offset_str);
        }
        write!(f, "{} * X{}{}", self.coeff, self.var, offset_str)
    }
}

pub fn get_affine_expr(expr: &LinearExpressionProto) -> AffineExpr {
    assert!(expr.vars.len() <= 1);
    if expr.vars.is_empty() {
        return AffineExpr::const_(expr.offset);
    }
    assert_eq!(expr.coeffs.len(), 1);
    AffineExpr {
        var: expr.vars[0],
        coeff: expr.coeffs[0],
        offset: expr.offset,
    }
}

// Evaluates the minimum and maximum possible values of an affine expression
// given the variable domains in the CpModelProto.
pub fn get_affine_expr_min(expr: &AffineExpr, model_proto: &CpModelProto) -> i64 {
    debug_assert!(expr.var <= model_proto.variables.len() as i32);
    if expr.var < 0 {
        return expr.offset;
    }
    let var_proto = &model_proto.variables[expr.var as usize];
    assert!(!var_proto.domain.is_empty());
    if expr.coeff >= 0 {
        let d_min = var_proto.domain[0];
        expr.coeff * d_min + expr.offset
    } else {
        let d_max = var_proto.domain[var_proto.domain.len() - 1];
        expr.coeff * d_max + expr.offset
    }
}

pub fn get_affine_expr_max(expr: &AffineExpr, model_proto: &CpModelProto) -> i64 {
    debug_assert!(expr.var <= model_proto.variables.len() as i32);
    if expr.var < 0 {
        return expr.offset;
    }
    let var_proto = &model_proto.variables[expr.var as usize];
    assert!(!var_proto.domain.is_empty());
    if expr.coeff >= 0 {
        let d_max = var_proto.domain[var_proto.domain.len() - 1];
        expr.coeff * d_max + expr.offset
    } else {
        let d_min = var_proto.domain[0];
        expr.coeff * d_min + expr.offset
    }
}

// Returns the gcd of the given LinearExpressionProto.
// Specifying the second argument will take the gcd with it.
pub fn linear_expression_gcd(expr: &LinearExpressionProto, gcd: i64) -> i64 {
    // `std::abs` at sat/cp_model_utils.cc:77 gives the smallest int64 back for the smallest
    // int64, as the reference build shows: its gcd with 0 is the smallest int64.
    let mut gcd = std_gcd(gcd, expr.offset.wrapping_abs());
    for &coeff in &expr.coeffs {
        gcd = std_gcd(gcd, coeff.wrapping_abs());
    }
    gcd
}

// `std::gcd`: the greatest common divisor of the absolute values, 0 for (0, 0).
pub(crate) fn std_gcd(m: i64, n: i64) -> i64 {
    let (mut a, mut b) = (m.unsigned_abs(), n.unsigned_abs());
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a as i64
}

// Divide the expression in place by 'divisor'.
// It will DCHECK that 'divisor' divides all constants.
pub fn divide_linear_expression(divisor: i64, expr: &mut LinearExpressionProto) {
    assert_ne!(divisor, 0);
    if divisor == 1 {
        return;
    }

    debug_assert_eq!(expr.offset % divisor, 0);
    expr.offset /= divisor;
    for i in 0..expr.vars.len() {
        debug_assert_eq!(expr.coeffs[i] % divisor, 0);
        expr.coeffs[i] /= divisor;
    }
}

// Fills the target as negated ref.
pub fn set_to_negated_linear_expression(
    input_expr: &LinearExpressionProto,
    output_negated_expr: &mut LinearExpressionProto,
) {
    *output_negated_expr = LinearExpressionProto::default();
    for i in 0..input_expr.vars.len() {
        output_negated_expr
            .vars
            .push(negated_ref(input_expr.vars[i]));
        output_negated_expr.coeffs.push(input_expr.coeffs[i]);
    }
    output_negated_expr.offset = -input_expr.offset;
}

// Collects all the references used by a constraint. This function is used in a
// few places to have a "generic" code dealing with constraints. Note that the
// enforcement_literal is NOT counted here and that the vectors can have
// duplicates.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct IndexReferences {
    pub variables: Vec<i32>,
    pub literals: Vec<i32>,
}

fn add_indices(indices: &[i32], output: &mut Vec<i32>) {
    output.extend_from_slice(indices);
}

fn add_expression_indices<'a>(
    exprs: impl IntoIterator<Item = &'a LinearExpressionProto>,
    output: &mut Vec<i32>,
) {
    for expr in exprs {
        add_indices(&expr.vars, output);
    }
}

pub fn get_references_used_by_constraint(ct: &ConstraintProto) -> IndexReferences {
    let mut output = IndexReferences::default();
    get_references_used_by_constraint_into(ct, &mut output.variables, &mut output.literals);
    output
}

pub fn get_references_used_by_constraint_into(
    ct: &ConstraintProto,
    variables: &mut Vec<i32>,
    literals: &mut Vec<i32>,
) {
    variables.clear();
    literals.clear();
    let Some(constraint) = &ct.constraint else {
        return;
    };
    match constraint {
        Constraint::BoolOr(arg) => add_indices(&arg.literals, literals),
        Constraint::BoolAnd(arg) => add_indices(&arg.literals, literals),
        Constraint::AtMostOne(arg) => add_indices(&arg.literals, literals),
        Constraint::ExactlyOne(arg) => add_indices(&arg.literals, literals),
        Constraint::BoolXor(arg) => add_indices(&arg.literals, literals),
        Constraint::IntDiv(arg) => {
            add_expression_indices(&arg.target, variables);
            add_expression_indices(&arg.exprs, variables);
        }
        Constraint::IntMod(arg) => {
            add_expression_indices(&arg.target, variables);
            add_expression_indices(&arg.exprs, variables);
        }
        Constraint::LinMax(arg) => {
            add_expression_indices(&arg.target, variables);
            add_expression_indices(&arg.exprs, variables);
        }
        Constraint::IntProd(arg) => {
            add_expression_indices(&arg.target, variables);
            add_expression_indices(&arg.exprs, variables);
        }
        Constraint::Linear(arg) => add_indices(&arg.vars, variables),
        Constraint::AllDiff(arg) => add_expression_indices(&arg.exprs, variables),
        Constraint::DummyConstraint(arg) => add_indices(&arg.vars, variables),
        Constraint::Element(element) => {
            if element.index != 0 || element.target != 0 || !element.vars.is_empty() {
                variables.push(element.index);
                variables.push(element.target);
                add_indices(&element.vars, variables);
            } else if element.linear_index.is_some()
                || element.linear_target.is_some()
                || !element.exprs.is_empty()
            {
                add_expression_indices(&element.linear_index, variables);
                add_expression_indices(&element.linear_target, variables);
                add_expression_indices(&element.exprs, variables);
            }
        }
        Constraint::Circuit(arg) => add_indices(&arg.literals, literals),
        Constraint::Routes(arg) => {
            add_indices(&arg.literals, literals);
            // The node expressions are not used by the constraint itself.
        }
        Constraint::Inverse(arg) => {
            if !arg.f_direct.is_empty() {
                add_indices(&arg.f_direct, variables);
                add_indices(&arg.f_inverse, variables);
            } else {
                add_expression_indices(&arg.f_expr_direct, variables);
                add_expression_indices(&arg.f_expr_inverse, variables);
            }
        }
        Constraint::Reservoir(arg) => {
            add_expression_indices(&arg.time_exprs, variables);
            add_expression_indices(&arg.level_changes, variables);
            add_indices(&arg.active_literals, literals);
        }
        Constraint::Table(arg) => {
            if !arg.vars.is_empty() {
                add_indices(&arg.vars, variables);
            } else {
                add_expression_indices(&arg.exprs, variables);
            }
        }
        Constraint::Automaton(arg) => {
            if !arg.vars.is_empty() {
                add_indices(&arg.vars, variables);
            } else {
                add_expression_indices(&arg.exprs, variables);
            }
        }
        Constraint::Interval(arg) => {
            add_expression_indices(&arg.start, variables);
            add_expression_indices(&arg.size, variables);
            add_expression_indices(&arg.end, variables);
        }
        Constraint::NoOverlap(_) => {}
        Constraint::NoOverlap2d(_) => {}
        Constraint::Cumulative(arg) => {
            add_expression_indices(&arg.capacity, variables);
            add_expression_indices(&arg.demands, variables);
        }
    }
}

// Applies `f` to each element of a repeated field.
fn apply_to_repeated(field: &mut [i32], f: &mut impl FnMut(&mut i32)) {
    for r in field {
        f(r);
    }
}

// Applies `f` to the `vars` of `target` and of each expression. `target` is created when
// it is absent, as the C++ `mutable_target()` does.
fn apply_to_target_and_exprs(
    target: &mut Option<LinearExpressionProto>,
    exprs: &mut [LinearExpressionProto],
    f: &mut impl FnMut(&mut i32),
) {
    apply_to_repeated(&mut target.get_or_insert_default().vars, f);
    apply_to_exprs(exprs, f);
}

fn apply_to_exprs(exprs: &mut [LinearExpressionProto], f: &mut impl FnMut(&mut i32)) {
    for expr in exprs {
        apply_to_repeated(&mut expr.vars, f);
    }
}

// Applies the given function to all variables/literals/intervals indices of the
// constraint. This function is used in a few places to have a "generic" code
// dealing with constraints.
pub fn apply_to_all_variable_indices(mut f: impl FnMut(&mut i32), ct: &mut ConstraintProto) {
    let f = &mut f;
    let Some(constraint) = &mut ct.constraint else {
        return;
    };
    match constraint {
        Constraint::BoolOr(_) => {}
        Constraint::BoolAnd(_) => {}
        Constraint::AtMostOne(_) => {}
        Constraint::ExactlyOne(_) => {}
        Constraint::BoolXor(_) => {}
        Constraint::IntDiv(arg) => apply_to_target_and_exprs(&mut arg.target, &mut arg.exprs, f),
        Constraint::IntMod(arg) => apply_to_target_and_exprs(&mut arg.target, &mut arg.exprs, f),
        Constraint::LinMax(arg) => apply_to_target_and_exprs(&mut arg.target, &mut arg.exprs, f),
        Constraint::IntProd(arg) => apply_to_target_and_exprs(&mut arg.target, &mut arg.exprs, f),
        Constraint::Linear(arg) => apply_to_repeated(&mut arg.vars, f),
        Constraint::AllDiff(arg) => apply_to_exprs(&mut arg.exprs, f),
        Constraint::DummyConstraint(arg) => apply_to_repeated(&mut arg.vars, f),
        Constraint::Element(element) => {
            if element.index != 0 || element.target != 0 || !element.vars.is_empty() {
                f(&mut element.index);
                f(&mut element.target);
                apply_to_repeated(&mut element.vars, f);
            } else if element.linear_index.is_some()
                || element.linear_target.is_some()
                || !element.exprs.is_empty()
            {
                apply_to_repeated(&mut element.linear_index.get_or_insert_default().vars, f);
                apply_to_repeated(&mut element.linear_target.get_or_insert_default().vars, f);
                apply_to_exprs(&mut element.exprs, f);
            }
        }
        Constraint::Circuit(_) => {}
        Constraint::Routes(_) => {}
        Constraint::Inverse(arg) => {
            if !arg.f_direct.is_empty() {
                apply_to_repeated(&mut arg.f_direct, f);
                apply_to_repeated(&mut arg.f_inverse, f);
            } else {
                apply_to_exprs(&mut arg.f_expr_direct, f);
                apply_to_exprs(&mut arg.f_expr_inverse, f);
            }
        }
        Constraint::Reservoir(arg) => {
            apply_to_exprs(&mut arg.time_exprs, f);
            apply_to_exprs(&mut arg.level_changes, f);
        }
        Constraint::Table(arg) => {
            if !arg.vars.is_empty() {
                apply_to_repeated(&mut arg.vars, f);
            } else {
                apply_to_exprs(&mut arg.exprs, f);
            }
        }
        Constraint::Automaton(arg) => {
            if !arg.vars.is_empty() {
                apply_to_repeated(&mut arg.vars, f);
            } else {
                apply_to_exprs(&mut arg.exprs, f);
            }
        }
        Constraint::Interval(arg) => {
            apply_to_repeated(&mut arg.start.get_or_insert_default().vars, f);
            apply_to_repeated(&mut arg.size.get_or_insert_default().vars, f);
            apply_to_repeated(&mut arg.end.get_or_insert_default().vars, f);
        }
        Constraint::NoOverlap(_) => {}
        Constraint::NoOverlap2d(_) => {}
        Constraint::Cumulative(arg) => {
            apply_to_repeated(&mut arg.capacity.get_or_insert_default().vars, f);
            apply_to_exprs(&mut arg.demands, f);
        }
    }
}

pub fn apply_to_all_literal_indices(mut f: impl FnMut(&mut i32), ct: &mut ConstraintProto) {
    let f = &mut f;
    apply_to_repeated(&mut ct.enforcement_literal, f);
    let Some(constraint) = &mut ct.constraint else {
        return;
    };
    match constraint {
        Constraint::BoolOr(arg) => apply_to_repeated(&mut arg.literals, f),
        Constraint::BoolAnd(arg) => apply_to_repeated(&mut arg.literals, f),
        Constraint::AtMostOne(arg) => apply_to_repeated(&mut arg.literals, f),
        Constraint::ExactlyOne(arg) => apply_to_repeated(&mut arg.literals, f),
        Constraint::BoolXor(arg) => apply_to_repeated(&mut arg.literals, f),
        Constraint::IntDiv(_) => {}
        Constraint::IntMod(_) => {}
        Constraint::LinMax(_) => {}
        Constraint::IntProd(_) => {}
        Constraint::Linear(_) => {}
        Constraint::AllDiff(_) => {}
        Constraint::DummyConstraint(_) => {}
        Constraint::Element(_) => {}
        Constraint::Circuit(arg) => apply_to_repeated(&mut arg.literals, f),
        Constraint::Routes(arg) => apply_to_repeated(&mut arg.literals, f),
        Constraint::Inverse(_) => {}
        Constraint::Reservoir(arg) => apply_to_repeated(&mut arg.active_literals, f),
        Constraint::Table(_) => {}
        Constraint::Automaton(_) => {}
        Constraint::Interval(_) => {}
        Constraint::NoOverlap(_) => {}
        Constraint::NoOverlap2d(_) => {}
        Constraint::Cumulative(_) => {}
    }
}

pub fn apply_to_all_interval_indices(mut f: impl FnMut(&mut i32), ct: &mut ConstraintProto) {
    let f = &mut f;
    let Some(constraint) = &mut ct.constraint else {
        return;
    };
    match constraint {
        Constraint::BoolOr(_) => {}
        Constraint::BoolAnd(_) => {}
        Constraint::AtMostOne(_) => {}
        Constraint::ExactlyOne(_) => {}
        Constraint::BoolXor(_) => {}
        Constraint::IntDiv(_) => {}
        Constraint::IntMod(_) => {}
        Constraint::LinMax(_) => {}
        Constraint::IntProd(_) => {}
        Constraint::Linear(_) => {}
        Constraint::AllDiff(_) => {}
        Constraint::DummyConstraint(_) => {}
        Constraint::Element(_) => {}
        Constraint::Circuit(_) => {}
        Constraint::Routes(_) => {}
        Constraint::Inverse(_) => {}
        Constraint::Reservoir(_) => {}
        Constraint::Table(_) => {}
        Constraint::Automaton(_) => {}
        Constraint::Interval(_) => {}
        Constraint::NoOverlap(arg) => apply_to_repeated(&mut arg.intervals, f),
        Constraint::NoOverlap2d(arg) => {
            apply_to_repeated(&mut arg.x_intervals, f);
            apply_to_repeated(&mut arg.y_intervals, f);
        }
        Constraint::Cumulative(arg) => apply_to_repeated(&mut arg.intervals, f),
    }
}

// Applies the given function to all LinearExpressionProto fields of the
// constraint.
pub fn apply_to_all_linear_expressions(
    mut f: impl FnMut(&mut LinearExpressionProto),
    ct: &mut ConstraintProto,
) {
    let Some(constraint) = &mut ct.constraint else {
        return;
    };
    match constraint {
        Constraint::BoolOr(_)
        | Constraint::BoolAnd(_)
        | Constraint::AtMostOne(_)
        | Constraint::ExactlyOne(_)
        | Constraint::BoolXor(_)
        | Constraint::Linear(_)
        | Constraint::DummyConstraint(_)
        | Constraint::Circuit(_)
        | Constraint::NoOverlap(_)
        | Constraint::NoOverlap2d(_) => {}
        Constraint::IntDiv(proto)
        | Constraint::IntMod(proto)
        | Constraint::LinMax(proto)
        | Constraint::IntProd(proto) => {
            if let Some(target) = &mut proto.target {
                f(target);
            }
            for expr in &mut proto.exprs {
                f(expr);
            }
        }
        Constraint::AllDiff(proto) => {
            for expr in &mut proto.exprs {
                f(expr);
            }
        }
        Constraint::Element(elem) => {
            if elem.index == 0
                && elem.target == 0
                && elem.vars.is_empty()
                && (elem.linear_index.is_some()
                    || elem.linear_target.is_some()
                    || !elem.exprs.is_empty())
            {
                if let Some(linear_index) = &mut elem.linear_index {
                    f(linear_index);
                }
                if let Some(linear_target) = &mut elem.linear_target {
                    f(linear_target);
                }
                for expr in &mut elem.exprs {
                    f(expr);
                }
            }
        }
        Constraint::Routes(proto) => {
            for dim in &mut proto.dimensions {
                for expr in &mut dim.exprs {
                    f(expr);
                }
            }
        }
        Constraint::Inverse(proto) => {
            if proto.f_direct.is_empty() && proto.f_inverse.is_empty() {
                for expr in &mut proto.f_expr_direct {
                    f(expr);
                }
                for expr in &mut proto.f_expr_inverse {
                    f(expr);
                }
            }
        }
        Constraint::Reservoir(proto) => {
            for expr in &mut proto.time_exprs {
                f(expr);
            }
            for expr in &mut proto.level_changes {
                f(expr);
            }
        }
        Constraint::Table(proto) => {
            if proto.vars.is_empty() {
                for expr in &mut proto.exprs {
                    f(expr);
                }
            }
        }
        Constraint::Automaton(proto) => {
            if proto.vars.is_empty() {
                for expr in &mut proto.exprs {
                    f(expr);
                }
            }
        }
        Constraint::Interval(proto) => {
            if let Some(start) = &mut proto.start {
                f(start);
            }
            if let Some(size) = &mut proto.size {
                f(size);
            }
            if let Some(end) = &mut proto.end {
                f(end);
            }
        }
        Constraint::Cumulative(proto) => {
            if let Some(capacity) = &mut proto.capacity {
                f(capacity);
            }
            for expr in &mut proto.demands {
                f(expr);
            }
        }
    }
}

// The overload of `apply_to_all_linear_expressions` for a constant constraint.
pub fn apply_to_all_linear_expressions_const(
    mut f: impl FnMut(&LinearExpressionProto),
    ct: &ConstraintProto,
) {
    let Some(constraint) = &ct.constraint else {
        return;
    };
    match constraint {
        Constraint::BoolOr(_)
        | Constraint::BoolAnd(_)
        | Constraint::AtMostOne(_)
        | Constraint::ExactlyOne(_)
        | Constraint::BoolXor(_)
        | Constraint::Linear(_)
        | Constraint::DummyConstraint(_)
        | Constraint::Circuit(_)
        | Constraint::NoOverlap(_)
        | Constraint::NoOverlap2d(_) => {}
        Constraint::IntDiv(proto)
        | Constraint::IntMod(proto)
        | Constraint::LinMax(proto)
        | Constraint::IntProd(proto) => {
            if let Some(target) = &proto.target {
                f(target);
            }
            for expr in &proto.exprs {
                f(expr);
            }
        }
        Constraint::AllDiff(proto) => {
            for expr in &proto.exprs {
                f(expr);
            }
        }
        Constraint::Element(elem) => {
            if elem.index == 0
                && elem.target == 0
                && elem.vars.is_empty()
                && (elem.linear_index.is_some()
                    || elem.linear_target.is_some()
                    || !elem.exprs.is_empty())
            {
                if let Some(linear_index) = &elem.linear_index {
                    f(linear_index);
                }
                if let Some(linear_target) = &elem.linear_target {
                    f(linear_target);
                }
                for expr in &elem.exprs {
                    f(expr);
                }
            }
        }
        Constraint::Routes(proto) => {
            for dim in &proto.dimensions {
                for expr in &dim.exprs {
                    f(expr);
                }
            }
        }
        Constraint::Inverse(proto) => {
            if proto.f_direct.is_empty() && proto.f_inverse.is_empty() {
                for expr in &proto.f_expr_direct {
                    f(expr);
                }
                for expr in &proto.f_expr_inverse {
                    f(expr);
                }
            }
        }
        Constraint::Reservoir(proto) => {
            for expr in &proto.time_exprs {
                f(expr);
            }
            for expr in &proto.level_changes {
                f(expr);
            }
        }
        Constraint::Table(proto) => {
            if proto.vars.is_empty() {
                for expr in &proto.exprs {
                    f(expr);
                }
            }
        }
        Constraint::Automaton(proto) => {
            if proto.vars.is_empty() {
                for expr in &proto.exprs {
                    f(expr);
                }
            }
        }
        Constraint::Interval(proto) => {
            if let Some(start) = &proto.start {
                f(start);
            }
            if let Some(size) = &proto.size {
                f(size);
            }
            if let Some(end) = &proto.end {
                f(end);
            }
        }
        Constraint::Cumulative(proto) => {
            if let Some(capacity) = &proto.capacity {
                f(capacity);
            }
            for expr in &proto.demands {
                f(expr);
            }
        }
    }
}

// Returns the sorted list of variables used by a constraint.
// Note that this includes variables used as literals.
pub fn used_variables(ct: &ConstraintProto) -> Vec<i32> {
    // The C++ call passes one vector for both outputs. Rust cannot alias two `&mut`, so the
    // two lists are joined; the sort below gives the same result.
    let mut result = Vec::new();
    let mut literals = Vec::new();
    get_references_used_by_constraint_into(ct, &mut result, &mut literals);
    result.append(&mut literals);
    for r in &mut result {
        *r = positive_ref(*r);
    }
    for &lit in &ct.enforcement_literal {
        result.push(positive_ref(lit));
    }
    result.sort_unstable();
    result.dedup();
    result
}

// Returns the sorted list of intervals used by a constraint.
pub fn used_intervals(ct: &ConstraintProto) -> Vec<i32> {
    let mut used_intervals = Vec::new();
    if let Some(constraint) = &ct.constraint {
        match constraint {
            Constraint::BoolOr(_) => {}
            Constraint::BoolAnd(_) => {}
            Constraint::AtMostOne(_) => {}
            Constraint::ExactlyOne(_) => {}
            Constraint::BoolXor(_) => {}
            Constraint::IntDiv(_) => {}
            Constraint::IntMod(_) => {}
            Constraint::LinMax(_) => {}
            Constraint::IntProd(_) => {}
            Constraint::Linear(_) => {}
            Constraint::AllDiff(_) => {}
            Constraint::DummyConstraint(_) => {}
            Constraint::Element(_) => {}
            Constraint::Circuit(_) => {}
            Constraint::Routes(_) => {}
            Constraint::Inverse(_) => {}
            Constraint::Reservoir(_) => {}
            Constraint::Table(_) => {}
            Constraint::Automaton(_) => {}
            Constraint::Interval(_) => {}
            Constraint::NoOverlap(arg) => add_indices(&arg.intervals, &mut used_intervals),
            Constraint::NoOverlap2d(arg) => {
                used_intervals.reserve(2 * arg.x_intervals.len());
                add_indices(&arg.x_intervals, &mut used_intervals);
                add_indices(&arg.y_intervals, &mut used_intervals);
            }
            Constraint::Cumulative(arg) => add_indices(&arg.intervals, &mut used_intervals),
        }
    }
    used_intervals.sort_unstable();
    used_intervals.dedup();
    used_intervals
}

// Insert/Remove variables from an interval constraint into a bitset.
pub fn insert_variables_from_interval(
    model_proto: &CpModelProto,
    index: i32,
    output: &mut Bitset64<i32>,
) {
    let ct = &model_proto.constraints[index as usize];
    for &r#ref in &ct.enforcement_literal {
        output.set(positive_ref(r#ref));
    }
    for var in interval_vars(ct) {
        output.set(var);
    }
}

pub fn remove_variables_from_interval(
    model_proto: &CpModelProto,
    index: i32,
    output: &mut Bitset64<i32>,
) {
    let ct = &model_proto.constraints[index as usize];
    for &r#ref in &ct.enforcement_literal {
        output.clear(positive_ref(r#ref));
    }
    for var in interval_vars(ct) {
        output.clear(var);
    }
}

// The variables of the start, size and end of `ct.interval()`, which is the empty
// default message when `ct` is not an interval.
fn interval_vars(ct: &ConstraintProto) -> impl Iterator<Item = i32> + '_ {
    let interval = match &ct.constraint {
        Some(Constraint::Interval(interval)) => Some(interval),
        _ => None,
    };
    [
        interval.and_then(|i| i.start.as_ref()),
        interval.and_then(|i| i.size.as_ref()),
        interval.and_then(|i| i.end.as_ref()),
    ]
    .into_iter()
    .flatten()
    .flat_map(|expr| expr.vars.iter().copied())
}

// The protos that hold a `domain` field: a sorted list of disjoint intervals.
pub trait ProtoWithDomain {
    fn domain(&self) -> &[i64];
    fn domain_mut(&mut self) -> &mut Vec<i64>;
}

macro_rules! impl_proto_with_domain {
    ($($proto:ty),+) => {
        $(
            impl ProtoWithDomain for $proto {
                fn domain(&self) -> &[i64] {
                    &self.domain
                }

                fn domain_mut(&mut self) -> &mut Vec<i64> {
                    &mut self.domain
                }
            }
        )+
    };
}

impl_proto_with_domain!(
    IntegerVariableProto,
    LinearConstraintProto,
    CpObjectiveProto
);

// Returns true if a proto.domain() contains the given value.
// The domain is expected to be encoded as a sorted disjoint interval list.
pub fn domain_in_proto_contains<P: ProtoWithDomain>(proto: &P, value: i64) -> bool {
    let domain = proto.domain();
    for i in (0..domain.len()).step_by(2) {
        if value >= domain[i] && value <= domain[i + 1] {
            return true;
        }
    }
    false
}

// Serializes a Domain into the domain field of a proto.
pub fn fill_domain_in_proto<P: ProtoWithDomain>(domain: &Domain, proto: &mut P) {
    let proto_domain = proto.domain_mut();
    proto_domain.clear();
    proto_domain.reserve(2 * domain.num_intervals() as usize);
    for interval in domain {
        proto_domain.push(interval.start);
        proto_domain.push(interval.end);
    }
}

pub fn fill_domain_in_proto_bounds<P: ProtoWithDomain>(lb: i64, ub: i64, proto: &mut P) {
    let proto_domain = proto.domain_mut();
    proto_domain.clear();
    proto_domain.reserve(2);
    proto_domain.push(lb);
    proto_domain.push(ub);
}

pub fn fill_domain_in_proto_value<P: ProtoWithDomain>(value: i64, proto: &mut P) {
    let proto_domain = proto.domain_mut();
    proto_domain.clear();
    proto_domain.reserve(2);
    proto_domain.push(value);
    proto_domain.push(value);
}

// Reads a Domain from the domain field of a proto.
pub fn read_domain_from_proto<P: ProtoWithDomain>(proto: &P) -> Domain {
    Domain::from_flat_span_of_intervals(proto.domain())
}

// Returns the list of values in a given domain.
// This will fail if the domain contains more than one million values.
//
// TODO(user): work directly on the Domain class instead.
pub fn all_values_in_domain<P: ProtoWithDomain>(proto: &P) -> Vec<i64> {
    let domain = proto.domain();
    let mut result = Vec::new();
    for i in (0..domain.len()).step_by(2) {
        // A range, unlike the C++ loop counter, does not overflow at the largest value.
        for v in domain[i]..=domain[i + 1] {
            assert!(result.len() as f64 <= 1e6);
            result.push(v);
        }
    }
    result
}

// Scales back an objective value to a double value from the original model.
pub fn scale_objective_value(proto: &CpObjectiveProto, value: i64) -> f64 {
    let mut result = value as f64;
    if value == i64::MIN {
        result = f64::NEG_INFINITY;
    }
    if value == i64::MAX {
        result = f64::INFINITY;
    }
    result += proto.offset;
    if proto.scaling_factor == 0.0 {
        return result;
    }
    proto.scaling_factor * result
}

// Removes the objective scaling and offset from the given value.
pub fn unscale_objective_value(proto: &CpObjectiveProto, value: f64) -> f64 {
    let mut result = value;
    if proto.scaling_factor != 0.0 {
        result /= proto.scaling_factor;
    }
    result - proto.offset
}

// Transforms an inner objective value to an "outer" one (original value before
// presolve). Note that the "outer" objective here refers to the integer
// expression of the objective before presolve, but not counting the objective
// offset and/or scaling. So this is not completely in the user-domain.
pub fn postsolve_inner_objective_value(proto: &CpObjectiveProto, value: i64) -> i64 {
    if value == i64::MIN || value == i64::MAX {
        return value;
    }
    if proto.integer_scaling_factor == 0 {
        return value + proto.integer_before_offset;
    }
    (value + proto.integer_before_offset) * proto.integer_scaling_factor
        + proto.integer_after_offset
}

// Inverse of PostsolveInnerObjectiveValue(). See the comments above.
pub fn presolve_inner_objective_value(proto: &CpObjectiveProto, value: i64) -> i64 {
    if proto.integer_scaling_factor == 0 {
        return value - proto.integer_before_offset;
    }
    (value - proto.integer_after_offset) / proto.integer_scaling_factor
        - proto.integer_before_offset
}

// Computes the "inner" objective of a response that contains a solution.
// This is the objective without offset and scaling. Call ScaleObjectiveValue()
// to get the user-facing objective.
pub fn compute_inner_objective(objective: &CpObjectiveProto, solution: &[i64]) -> i64 {
    let mut objective_value: i64 = 0;
    for i in 0..objective.vars.len() {
        let mut coeff = objective.coeffs[i];
        let r#ref = objective.vars[i];
        let var = positive_ref(r#ref);
        if !ref_is_positive(r#ref) {
            coeff = -coeff;
        }
        objective_value += coeff * solution[var as usize];
    }
    objective_value
}

pub fn compute_float_objective_value(proto: &FloatObjectiveProto, solution: &[i64]) -> f64 {
    let mut sum = TightSum::default();
    sum.add(proto.offset);
    for i in 0..proto.vars.len() {
        sum.add_prod(proto.coeffs[i], solution[proto.vars[i] as usize]);
    }
    if proto.maximize {
        sum.tight_lower_bound()
    } else {
        sum.tight_upper_bound()
    }
}

// Returns true if a linear expression can be reduced to a single ref.
// That is -var or +var.
pub fn expression_contains_single_ref(expr: &LinearExpressionProto) -> bool {
    // `std::abs` at sat/cp_model_utils.cc:868 gives the smallest int64 back for the smallest
    // int64, so the reference build says such a coefficient is not a single ref.
    expr.offset == 0 && expr.vars.len() == 1 && expr.coeffs[0].wrapping_abs() == 1
}

// Checks if the expression is affine or constant.
pub fn expression_is_affine(expr: &LinearExpressionProto) -> bool {
    expr.vars.len() <= 1
}

// Returns the reference the expression can be reduced to. It will DCHECK that
// ExpressionContainsSingleRef(expr) is true.
pub fn get_single_ref_from_expression(expr: &LinearExpressionProto) -> i32 {
    debug_assert!(expression_contains_single_ref(expr));
    if expr.coeffs[0] == 1 {
        expr.vars[0]
    } else {
        negated_ref(expr.vars[0])
    }
}

// Evaluates an affine expression at the given value.
pub fn affine_expression_value_at(expr: &LinearExpressionProto, value: i64) -> i64 {
    assert_eq!(1, expr.vars.len());
    expr.offset + value * expr.coeffs[0]
}

// Returns the value of the inner variable of an affine expression from the
// value of the expression. It will DCHECK that the result is valid.
pub fn get_inner_var_value(expr: &LinearExpressionProto, value: i64) -> i64 {
    debug_assert_eq!(expr.vars.len(), 1);
    let var_value = (value - expr.offset) / expr.coeffs[0];
    debug_assert_eq!(value, var_value * expr.coeffs[0] + expr.offset);
    var_value
}

// Returns true if the expression is a * var + b.
pub fn affine_expression_contains_var(expr: &LinearExpressionProto, var: i32) -> bool {
    expr.vars.len() == 1 && expr.vars[0] == var
}

// Adds a linear expression proto to a linear constraint in place.
//
// Important: The domain must already be set, otherwise the offset will be lost.
// We also do not do any duplicate detection, so the constraint might need
// presolving afterwards.
pub fn add_linear_expression_to_linear_constraint(
    expr: &LinearExpressionProto,
    coefficient: i64,
    linear: &mut LinearConstraintProto,
) {
    for i in 0..expr.vars.len() {
        linear.vars.push(expr.vars[i]);
        linear.coeffs.push(expr.coeffs[i] * coefficient);
    }
    debug_assert!(!linear.domain.is_empty());
    let shift = coefficient * expr.offset;
    if shift != 0 {
        let domain = read_domain_from_proto(linear).addition_with(&Domain::new_value(-shift));
        fill_domain_in_proto(&domain, linear);
    }
}

// Same as above, but with a single term (lit, coeff). Note that lit can be
// negative. The offset is relative to the linear expression (and should be
// negated when added to the rhs of the linear constraint proto).
pub fn add_weighted_literal_to_linear_constraint(
    lit: i32,
    coeff: i64,
    linear: &mut LinearConstraintProto,
    offset: &mut i64,
) {
    if coeff == 0 {
        return;
    }
    if ref_is_positive(lit) {
        linear.vars.push(lit);
        linear.coeffs.push(coeff);
    } else {
        linear.vars.push(negated_ref(lit));
        linear.coeffs.push(-coeff);
        *offset += coeff;
    }
}

// Sets `linear` to the constraint "lb <= sum(`literals`) <= ub".
pub fn literals_to_linear(
    literals: &[i32],
    mut lb: i64,
    mut ub: i64,
    linear: &mut LinearConstraintProto,
) {
    *linear = LinearConstraintProto::default();
    for &lit in literals {
        if ref_is_positive(lit) {
            linear.vars.push(lit);
            linear.coeffs.push(1);
        } else {
            linear.vars.push(negated_ref(lit));
            linear.coeffs.push(-1);
            lb -= 1;
            ub -= 1;
        }
    }
    linear.domain.push(lb);
    linear.domain.push(ub);
}

// Same method, but returns if the addition was possible without overflowing.
pub fn safe_add_linear_expression_to_linear_constraint(
    expr: &LinearExpressionProto,
    coefficient: i64,
    linear: &mut LinearConstraintProto,
) -> bool {
    for i in 0..expr.vars.len() {
        linear.vars.push(expr.vars[i]);
        let prod = cap_prod(expr.coeffs[i], coefficient);
        if at_min_or_max_int64(prod) {
            return false;
        }
        linear.coeffs.push(prod);
    }
    debug_assert!(!linear.domain.is_empty());

    let shift = cap_prod(coefficient, expr.offset);
    if at_min_or_max_int64(shift) {
        return false;
    }
    let d = read_domain_from_proto(linear).addition_with(&Domain::new_value(-shift));
    if at_min_or_max_int64(d.min()) || at_min_or_max_int64(d.max()) {
        return false;
    }
    fill_domain_in_proto(&d, linear);
    true
}

// Returns if a constraint is of the form y = lin_max(x, -x).
pub fn is_affine_int_abs(ct: &ConstraintProto) -> bool {
    let Some(Constraint::LinMax(lin_max)) = &ct.constraint else {
        return false;
    };
    if lin_max.exprs.len() != 2
        || lin_max.target.as_ref().is_some_and(|t| t.vars.len() > 1)
        || lin_max.exprs[0].vars.len() != 1
        || lin_max.exprs[1].vars.len() != 1
    {
        return false;
    }

    if lin_max.exprs[0].offset != -lin_max.exprs[1].offset {
        return false;
    }
    if positive_ref(lin_max.exprs[0].vars[0]) != positive_ref(lin_max.exprs[1].vars[0]) {
        return false;
    }

    let left_coeff = if ref_is_positive(lin_max.exprs[0].vars[0]) {
        lin_max.exprs[0].coeffs[0]
    } else {
        -lin_max.exprs[0].coeffs[0]
    };
    let right_coeff = if ref_is_positive(lin_max.exprs[1].vars[0]) {
        lin_max.exprs[1].coeffs[0]
    } else {
        -lin_max.exprs[1].coeffs[0]
    };
    left_coeff == -right_coeff
}

// Returns true iff a == b * b_scaling. Note that this relies on a hash-map and
// does not care about the order of the terms.
pub fn linear_expression_protos_are_equal(
    a: &LinearExpressionProto,
    b: &LinearExpressionProto,
    b_scaling: i64,
) -> bool {
    if a.vars.len() != b.vars.len() {
        return false;
    }
    if a.offset != b.offset * b_scaling {
        return false;
    }
    let mut coeffs: HashMap<i32, i64> = HashMap::new();
    for i in 0..a.vars.len() {
        *coeffs.entry(a.vars[i]).or_default() += a.coeffs[i];
        *coeffs.entry(b.vars[i]).or_default() += -b.coeffs[i] * b_scaling;
    }

    for &coeff in coeffs.values() {
        if coeff != 0 {
            return false;
        }
    }
    true
}

// Contrary to LinearExpressionProtosAreEqual(), this does not use hash_map.
pub fn linear_expression_protos_are_exactly_equal(
    a: &LinearExpressionProto,
    b: &LinearExpressionProto,
) -> bool {
    a.vars == b.vars && a.coeffs == b.coeffs && a.offset == b.offset
}

// Compares two LinearExpressionProtos lexicographically by (vars, coeffs,
// offset).
pub fn linear_expression_proto_less_than(
    a: &LinearExpressionProto,
    b: &LinearExpressionProto,
) -> bool {
    if a.vars != b.vars {
        return a.vars < b.vars;
    }
    if a.coeffs != b.coeffs {
        return a.coeffs < b.coeffs;
    }
    a.offset < b.offset
}

// Returns true if the expression has unique positive variables sorted by vars,
// non-zero coefficients, and no variable in enforcements.
pub fn linear_expression_is_canonical(
    enforcements: &HashSet<i32>,
    expr: &LinearExpressionProto,
) -> bool {
    if expr.vars.len() != expr.coeffs.len() {
        return false;
    }
    for i in 0..expr.vars.len() {
        let var = expr.vars[i];
        if !ref_is_positive(var) || expr.coeffs[i] == 0 {
            return false;
        }
        if i > 0 && expr.vars[i - 1] >= var {
            return false;
        }
        if enforcements.contains(&var) || enforcements.contains(&negated_ref(var)) {
            return false;
        }
    }
    true
}

pub fn linear_expression_is_canonical_without_enforcements(expr: &LinearExpressionProto) -> bool {
    linear_expression_is_canonical(&HashSet::new(), expr)
}

// Returns true if the lin_max constraint is canonical: its enforcement
// literals are sorted and unique (with distinct variables) and do not appear
// in target or exprs, and its lin_max argument is canonical.
pub fn lin_max_is_canonical(ct: &ConstraintProto) -> bool {
    let Some(Constraint::LinMax(lin_max)) = &ct.constraint else {
        return false;
    };
    for i in 1..ct.enforcement_literal.len() {
        if ct.enforcement_literal[i - 1] >= ct.enforcement_literal[i] {
            return false;
        }
    }
    if sorted_literals_have_opposite_pair(&ct.enforcement_literal) {
        return false;
    }
    let enforcements: HashSet<i32> = ct.enforcement_literal.iter().copied().collect();
    // The target of the C++ `lin_max.target()` is the empty default message when it is unset.
    let default_target = LinearExpressionProto::default();
    let target = lin_max.target.as_ref().unwrap_or(&default_target);
    if !linear_expression_is_canonical(&enforcements, target) {
        return false;
    }
    for expr in &lin_max.exprs {
        if !linear_expression_is_canonical(&enforcements, expr) {
            return false;
        }
    }
    lin_max
        .exprs
        .is_sorted_by(|a, b| !linear_expression_proto_less_than(b, a))
}

// The messages that hold a list of variables and the list of their coefficients.
pub trait ProtoWithVarsAndCoeffs {
    fn vars(&self) -> &[i32];
    fn coeffs(&self) -> &[i64];
    fn clear_vars_and_coeffs(&mut self);
    fn add_var_and_coeff(&mut self, var: i32, coeff: i64);
}

macro_rules! impl_proto_with_vars_and_coeffs {
    ($($proto:ty),+) => {
        $(
            impl ProtoWithVarsAndCoeffs for $proto {
                fn vars(&self) -> &[i32] {
                    &self.vars
                }

                fn coeffs(&self) -> &[i64] {
                    &self.coeffs
                }

                fn clear_vars_and_coeffs(&mut self) {
                    self.vars.clear();
                    self.coeffs.clear();
                }

                fn add_var_and_coeff(&mut self, var: i32, coeff: i64) {
                    self.vars.push(var);
                    self.coeffs.push(coeff);
                }
            }
        )+
    };
}

impl_proto_with_vars_and_coeffs!(LinearExpressionProto, LinearConstraintProto);

fn canonicalize_linear_terms<P: ProtoWithVarsAndCoeffs>(
    enforcements: &HashSet<i32>,
    proto: &mut P,
) -> i64 {
    let mut sum_of_fixed_terms: i64 = 0;
    let mut terms: Vec<(i32, i64)> = Vec::with_capacity(proto.vars().len());
    debug_assert_eq!(proto.vars().len(), proto.coeffs().len());
    for i in 0..proto.vars().len() {
        let var = proto.vars()[i];
        debug_assert!(ref_is_positive(var));
        let coeff = proto.coeffs()[i];
        if coeff == 0 {
            continue;
        }
        if !enforcements.is_empty() {
            if enforcements.contains(&var) {
                sum_of_fixed_terms += coeff;
                continue;
            }
            if enforcements.contains(&negated_ref(var)) {
                continue;
            }
        }
        terms.push((var, coeff));
    }
    terms.sort_unstable();
    proto.clear_vars_and_coeffs();
    let mut current_var: i32 = 0;
    let mut current_coeff: i64 = 0;
    for (var, coeff) in terms {
        if var == current_var {
            current_coeff += coeff;
        } else {
            if current_coeff != 0 {
                proto.add_var_and_coeff(current_var, current_coeff);
            }
            current_var = var;
            current_coeff = coeff;
        }
    }
    if current_coeff != 0 {
        proto.add_var_and_coeff(current_var, current_coeff);
    }
    sum_of_fixed_terms
}

// Sorts terms by variable, merges duplicate variables, removes zero
// coefficients, and replaces any variable in enforcements with its value.
pub fn canonicalize_linear_expression(
    enforcements: &HashSet<i32>,
    expr: &mut LinearExpressionProto,
) {
    if linear_expression_is_canonical(enforcements, expr) {
        return;
    }
    expr.offset += canonicalize_linear_terms(enforcements, expr);
}

pub fn canonicalize_linear_expression_without_enforcements(expr: &mut LinearExpressionProto) {
    canonicalize_linear_expression(&HashSet::new(), expr);
}

pub fn canonicalize_linear_expression_in_constraint(
    enforcements: &HashSet<i32>,
    proto: &mut LinearConstraintProto,
) {
    let offset = canonicalize_linear_terms(enforcements, proto);
    if offset != 0 {
        let domain = read_domain_from_proto(proto).addition_with(&Domain::new_value(-offset));
        fill_domain_in_proto(&domain, proto);
    }
}

// Canonicalizes the enforcement literals, target, and expressions of a
// lin_max constraint, and sorts exprs by (vars, coeffs, offset).
pub fn canonicalize_lin_max(ct: &mut ConstraintProto) {
    debug_assert!(matches!(ct.constraint, Some(Constraint::LinMax(_))));
    ct.enforcement_literal.sort_unstable();
    ct.enforcement_literal.dedup();
    if sorted_literals_have_opposite_pair(&ct.enforcement_literal) {
        *ct = ConstraintProto::default();
        return;
    }
    let enforcements: HashSet<i32> = ct.enforcement_literal.iter().copied().collect();
    // `mutable_lin_max()` switches the oneof to an empty lin_max, which is what a build without
    // the DCHECK above goes on with for another kind of constraint.
    if !matches!(ct.constraint, Some(Constraint::LinMax(_))) {
        ct.constraint = Some(Constraint::LinMax(LinearArgumentProto::default()));
    }
    let Some(Constraint::LinMax(lin_max)) = &mut ct.constraint else {
        unreachable!("the constraint was just made a lin_max");
    };
    canonicalize_linear_expression(&enforcements, lin_max.target.get_or_insert_default());
    for expr in &mut lin_max.exprs {
        canonicalize_linear_expression(&enforcements, expr);
    }
    lin_max.exprs.sort_unstable_by(|a, b| {
        if linear_expression_proto_less_than(a, b) {
            Ordering::Less
        } else if linear_expression_proto_less_than(b, a) {
            Ordering::Greater
        } else {
            Ordering::Equal
        }
    });
    debug_assert!(lin_max_is_canonical(ct));
}

// Returns true if there is exactly one variable appearing in all the
// expressions.
pub fn expressions_contains_only_one_var<'a>(
    exprs: impl IntoIterator<Item = &'a LinearExpressionProto>,
) -> bool {
    let mut unique_var: i32 = -1;
    for expr in exprs {
        for &var in &expr.vars {
            assert!(ref_is_positive(var));
            if unique_var == -1 {
                unique_var = var;
            } else if var != unique_var {
                return false;
            }
        }
    }
    unique_var != -1
}

// Default seed for fingerprints.
pub const K_DEFAULT_FINGERPRINT_SEED: u64 = 0xa5b85c5e198ed849;

// The types whose bytes in memory a fingerprint hashes, as the C++ `reinterpret_cast` of
// the field reads them.
pub trait NativeBytes {
    fn append_native_bytes(&self, bytes: &mut Vec<u8>);
}

macro_rules! impl_native_bytes {
    ($($number:ty),+) => {
        $(
            impl NativeBytes for $number {
                fn append_native_bytes(&self, bytes: &mut Vec<u8>) {
                    bytes.extend_from_slice(&self.to_ne_bytes());
                }
            }
        )+
    };
}

impl_native_bytes!(i32, i64, f64);

impl NativeBytes for bool {
    fn append_native_bytes(&self, bytes: &mut Vec<u8>) {
        bytes.push(u8::from(*self));
    }
}

pub fn fingerprint_repeated_field<T: NativeBytes>(sequence: &[T], seed: u64) -> u64 {
    if sequence.is_empty() {
        return seed;
    }
    let mut bytes = Vec::with_capacity(std::mem::size_of_val(sequence));
    for element in sequence {
        element.append_native_bytes(&mut bytes);
    }
    fasthash64(&bytes, seed)
}

pub fn fingerprint_single_field<T: NativeBytes>(field: &T, seed: u64) -> u64 {
    let mut bytes = Vec::with_capacity(std::mem::size_of::<T>());
    field.append_native_bytes(&mut bytes);
    fasthash64(&bytes, seed)
}

// Returns a stable fingerprint of a linear expression.
pub fn fingerprint_expression(lin: &LinearExpressionProto, seed: u64) -> u64 {
    let mut fp = seed;
    if !lin.vars.is_empty() {
        fp = fingerprint_repeated_field(&lin.vars, fp);
        fp = fingerprint_repeated_field(&lin.coeffs, fp);
    }
    fp = fingerprint_single_field(&lin.offset, fp);
    fp
}

// The C++ getter of an unset message field returns the empty default message.
fn or_empty(expr: &Option<LinearExpressionProto>) -> &LinearExpressionProto {
    static EMPTY: LazyLock<LinearExpressionProto> = LazyLock::new(LinearExpressionProto::default);
    expr.as_ref().unwrap_or(&EMPTY)
}

// Returns a stable fingerprint of a model.
pub fn fingerprint_model(model: &CpModelProto, seed: u64) -> u64 {
    let mut fp = seed;
    for var_proto in &model.variables {
        fp = fingerprint_repeated_field(&var_proto.domain, fp);
    }
    for ct in &model.constraints {
        if !ct.enforcement_literal.is_empty() {
            fp = fingerprint_repeated_field(&ct.enforcement_literal, fp);
        }
        let Some(constraint) = &ct.constraint else {
            continue;
        };
        match constraint {
            Constraint::BoolOr(arg) => fp = fingerprint_repeated_field(&arg.literals, fp),
            Constraint::BoolAnd(arg) => fp = fingerprint_repeated_field(&arg.literals, fp),
            Constraint::AtMostOne(arg) => fp = fingerprint_repeated_field(&arg.literals, fp),
            Constraint::ExactlyOne(arg) => fp = fingerprint_repeated_field(&arg.literals, fp),
            Constraint::BoolXor(arg) => fp = fingerprint_repeated_field(&arg.literals, fp),
            Constraint::IntDiv(arg)
            | Constraint::IntMod(arg)
            | Constraint::LinMax(arg)
            | Constraint::IntProd(arg) => {
                fp = fingerprint_expression(or_empty(&arg.target), fp);
                for expr in &arg.exprs {
                    fp = fingerprint_expression(expr, fp);
                }
            }
            Constraint::Linear(arg) => {
                fp = fingerprint_repeated_field(&arg.vars, fp);
                fp = fingerprint_repeated_field(&arg.coeffs, fp);
                fp = fingerprint_repeated_field(&arg.domain, fp);
            }
            Constraint::AllDiff(arg) => {
                for expr in &arg.exprs {
                    fp = fingerprint_expression(expr, fp);
                }
            }
            Constraint::DummyConstraint(_) => {}
            Constraint::Element(arg) => {
                fp = fingerprint_single_field(&arg.index, fp);
                fp = fingerprint_single_field(&arg.target, fp);
                fp = fingerprint_repeated_field(&arg.vars, fp);
                fp = fingerprint_expression(or_empty(&arg.linear_index), fp);
                fp = fingerprint_expression(or_empty(&arg.linear_target), fp);
                for expr in &arg.exprs {
                    fp = fingerprint_expression(expr, fp);
                }
            }
            Constraint::Circuit(arg) => {
                fp = fingerprint_repeated_field(&arg.heads, fp);
                fp = fingerprint_repeated_field(&arg.tails, fp);
                fp = fingerprint_repeated_field(&arg.literals, fp);
            }
            Constraint::Routes(arg) => {
                fp = fingerprint_repeated_field(&arg.heads, fp);
                fp = fingerprint_repeated_field(&arg.tails, fp);
                fp = fingerprint_repeated_field(&arg.literals, fp);
            }
            Constraint::Inverse(arg) => {
                if !arg.f_direct.is_empty() {
                    fp = fingerprint_repeated_field(&arg.f_direct, fp);
                    fp = fingerprint_repeated_field(&arg.f_inverse, fp);
                } else {
                    for expr in &arg.f_expr_direct {
                        fp = fingerprint_expression(expr, fp);
                    }
                    for expr in &arg.f_expr_inverse {
                        fp = fingerprint_expression(expr, fp);
                    }
                }
            }
            Constraint::Reservoir(arg) => {
                fp = fingerprint_single_field(&arg.min_level, fp);
                fp = fingerprint_single_field(&arg.max_level, fp);
                for expr in &arg.time_exprs {
                    fp = fingerprint_expression(expr, fp);
                }
                for expr in &arg.level_changes {
                    fp = fingerprint_expression(expr, fp);
                }
            }
            Constraint::Table(arg) => {
                if !arg.vars.is_empty() {
                    fp = fingerprint_repeated_field(&arg.vars, fp);
                } else {
                    for expr in &arg.exprs {
                        fp = fingerprint_expression(expr, fp);
                    }
                }
                fp = fingerprint_repeated_field(&arg.values, fp);
                fp = fingerprint_single_field(&arg.negated, fp);
            }
            Constraint::Automaton(arg) => {
                fp = fingerprint_single_field(&arg.starting_state, fp);
                fp = fingerprint_repeated_field(&arg.final_states, fp);
                fp = fingerprint_repeated_field(&arg.transition_tail, fp);
                fp = fingerprint_repeated_field(&arg.transition_head, fp);
                fp = fingerprint_repeated_field(&arg.transition_label, fp);
                if !arg.vars.is_empty() {
                    fp = fingerprint_repeated_field(&arg.vars, fp);
                } else {
                    for expr in &arg.exprs {
                        fp = fingerprint_expression(expr, fp);
                    }
                }
            }
            Constraint::Interval(arg) => {
                fp = fingerprint_expression(or_empty(&arg.start), fp);
                fp = fingerprint_expression(or_empty(&arg.size), fp);
                fp = fingerprint_expression(or_empty(&arg.end), fp);
            }
            Constraint::NoOverlap(arg) => fp = fingerprint_repeated_field(&arg.intervals, fp),
            Constraint::NoOverlap2d(arg) => {
                fp = fingerprint_repeated_field(&arg.x_intervals, fp);
                fp = fingerprint_repeated_field(&arg.y_intervals, fp);
            }
            Constraint::Cumulative(arg) => {
                fp = fingerprint_repeated_field(&arg.intervals, fp);
                fp = fingerprint_expression(or_empty(&arg.capacity), fp);
                for demand in &arg.demands {
                    fp = fingerprint_expression(demand, fp);
                }
            }
        }
    }

    // Fingerprint the objective.
    if let Some(objective) = &model.objective {
        fp = fingerprint_repeated_field(&objective.vars, fp);
        fp = fingerprint_repeated_field(&objective.coeffs, fp);
        fp = fingerprint_single_field(&objective.offset, fp);
        fp = fingerprint_single_field(&objective.scaling_factor, fp);
        fp = fingerprint_repeated_field(&objective.domain, fp);
    } else if let Some(objective) = &model.floating_point_objective {
        fp = fingerprint_repeated_field(&objective.vars, fp);
        fp = fingerprint_repeated_field(&objective.coeffs, fp);
        fp = fingerprint_single_field(&objective.offset, fp);
        fp = fingerprint_single_field(&objective.maximize, fp);
    }

    if let Some(solution_hint) = &model.solution_hint {
        fp = fingerprint_repeated_field(&solution_hint.vars, fp);
        fp = fingerprint_repeated_field(&solution_hint.values, fp);
    }

    // TODO(user): Should we fingerprint decision strategies?

    fp
}

// `SetupTextFormatPrinter` fills a C++ `TextFormat::Printer`, which has no Rust type, so the
// Rust entry is the printing function that uses it.
//
// Prints a model with the variables and the linear expressions on one line, and short
// repeated primitives, for example:
//
// variables { domain: [0, 1] }
// variables { domain: [0, 1] }
// variables { domain: [0, 1] }
//
// constraints {
//   linear {
//     vars: [0, 1, 2]
//     coeffs: [2, 4, 5 ]
//     domain: [11, 11]
//   }
// }
pub fn pretty_print_model_proto<M: prost::Message>(_proto: &M) -> String {
    unimplemented!("phase 2: text format printer for sat/cp_model_utils.cc")
}

pub fn write_model_proto_to_file<M: prost::Message>(proto: &M, filename: &str) -> bool {
    if filename.ends_with("txt") || filename.ends_with("textproto") {
        fs::write(filename, pretty_print_model_proto(proto)).is_ok()
    } else {
        fs::write(filename, proto.encode_to_vec()).is_ok()
    }
}

// Hashing support.
//
// Currently limited to a few inner types of ConstraintProto. The `operator==` and
// `AbslHashValue` of `BoolArgumentProto` and `LinearConstraintProto` are the derived
// `PartialEq` and `Hash` of the generated messages, which compare and hash the same fields.

pub fn convert_cp_model_proto_to_cnf(cp_model: &CpModelProto, out: &mut String) -> bool {
    out.clear();

    let num_vars = cp_model.variables.len();
    let mut num_clauses: i32 = 0;
    if !model_is_pure_sat(cp_model, &mut num_clauses) {
        return false;
    }

    out.push_str(&format!("p cnf {num_vars} {num_clauses}\n"));
    convert_sat_cp_model_proto_to_clauses(cp_model, |clause| {
        for lit in clause {
            out.push_str(&format!("{} ", lit.signed_value()));
        }
        out.push_str("0\n");
    });
    true
}

// This returns a wcnf model in the 2022 (new) format:
//     https://maxsat-evaluations.github.io/2022/rules.html
pub fn convert_cp_model_proto_to_wcnf(cp_model: &CpModelProto, out: &mut String) -> bool {
    out.clear();

    if !model_is_max_sat(cp_model) {
        return false;
    }

    convert_sat_cp_model_proto_to_clauses(cp_model, |clause| {
        out.push_str("h ");
        for lit in clause {
            out.push_str(&format!("{} ", lit.signed_value()));
        }
        out.push_str("0\n");
    });

    // Add the objective for MaxSAT problems.
    let obj = cp_model
        .objective
        .as_ref()
        .expect("a max-sat model has an objective");

    // MaxSAT is maximization, CP-SAT is minimization by default.
    // All weights must be > 0.
    //
    // If we maximize (b1 + b2) in CP-SAT, we will have
    // obj = minimize(-b1 - b2), scaling factor = -1.
    // In wcnf, we want 1 b1 0; 1 b2 0;
    //
    // If we minimize (b1 + b2), in CP-SAT, we will have
    // obj = b1 + b2, scaling factor = 1 (or not set).
    // In wcnf, we want 1 -b1 0; 1 -b2 0;
    //
    // Note that the objective displayed by a max-sat solve will thus not match
    // the real objective (it misses an offset and possibly a negation).
    for i in 0..obj.vars.len() {
        let opp_coeff = -obj.coeffs[i];
        let lit = Literal::new(
            BooleanVariable::new(positive_ref(obj.vars[i])),
            ref_is_positive(obj.vars[i]),
        );
        if opp_coeff > 0 {
            out.push_str(&format!("{} {} 0\n", opp_coeff, lit.signed_value()));
        } else if opp_coeff < 0 {
            out.push_str(&format!(
                "{} {} 0\n",
                -opp_coeff,
                lit.negated().signed_value()
            ));
        }
    }
    true
}

fn model_has_only_clauses_and_boolean_variables(
    cp_model: &CpModelProto,
    num_clauses: &mut i32,
) -> bool {
    let num_vars = cp_model.variables.len();
    *num_clauses = 0;
    for v in 0..num_vars {
        let domain = &cp_model.variables[v].domain;
        if domain.len() != 2 {
            return false;
        }
        let lb = domain[0];
        let ub = domain[1];
        if lb == ub {
            if lb != 0 && lb != 1 {
                return false;
            }
            *num_clauses += 1;
        } else if lb != 0 || ub != 1 {
            return false;
        }
    }
    for ct in &cp_model.constraints {
        match &ct.constraint {
            Some(Constraint::BoolOr(_)) => *num_clauses += 1,
            Some(Constraint::BoolAnd(arg)) => *num_clauses += arg.literals.len() as i32,
            _ => return false,
        }
    }
    true
}

fn model_is_max_sat(cp_model: &CpModelProto) -> bool {
    // We should only have bool_or and bool_and, and an integral objective.
    let mut num_clauses = 0;
    let Some(obj) = &cp_model.objective else {
        return false;
    };
    let scaling = if obj.scaling_factor == 0.0 {
        1.0
    } else {
        obj.scaling_factor
    };
    if scaling.round() != scaling {
        return false;
    }
    if obj.offset.round() != obj.offset {
        return false;
    }
    model_has_only_clauses_and_boolean_variables(cp_model, &mut num_clauses)
}

fn model_is_pure_sat(cp_model: &CpModelProto, num_clauses: &mut i32) -> bool {
    // We just ignore the objective if there is one, and the model should just
    // contain bool_or and bool_and.
    model_has_only_clauses_and_boolean_variables(cp_model, num_clauses)
}

fn convert_sat_cp_model_proto_to_clauses(
    cp_model: &CpModelProto,
    mut add_clause: impl FnMut(&[Literal]),
) {
    let num_vars = cp_model.variables.len();
    for v in 0..num_vars {
        let domain = &cp_model.variables[v].domain;
        let lb = domain[0];
        let ub = domain[1];
        if lb == ub {
            add_clause(&[Literal::new(BooleanVariable::new(v as i32), lb == 1)]);
        }
    }
    let literal = |lit: i32| {
        Literal::new(
            BooleanVariable::new(positive_ref(lit)),
            ref_is_positive(lit),
        )
    };
    let mut clause: Vec<Literal> = Vec::new();
    for ct in &cp_model.constraints {
        match &ct.constraint {
            Some(Constraint::BoolOr(arg)) => {
                clause.clear();
                for &lit in &ct.enforcement_literal {
                    clause.push(literal(lit).negated());
                }
                for &lit in &arg.literals {
                    clause.push(literal(lit));
                }
                add_clause(&clause);
            }
            Some(Constraint::BoolAnd(arg)) => {
                for &lit in &arg.literals {
                    clause.clear();
                    for &lit in &ct.enforcement_literal {
                        clause.push(literal(lit).negated());
                    }
                    clause.push(literal(lit));
                    add_clause(&clause);
                }
            }
            _ => {}
        }
    }
}

// We assume delta >= 0 and we only use the low bits of delta.
pub fn combine_seed(base_seed: i32, delta: i64) -> i32 {
    assert!(delta >= 0);
    let fp = fingerprint_single_field(&delta, K_DEFAULT_FINGERPRINT_SEED);
    (fingerprint_single_field(&base_seed, fp) & 0x7FFFFFFF) as i32
}

// The cases of the `ConstraintProto::ConstraintCase` oneof enum of the generated C++ code,
// numbered by the field number of the oneof member.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ConstraintCase {
    BoolOr = 3,
    BoolAnd = 4,
    AtMostOne = 26,
    ExactlyOne = 29,
    BoolXor = 5,
    IntDiv = 7,
    IntMod = 8,
    LinMax = 27,
    IntProd = 11,
    Linear = 12,
    AllDiff = 13,
    DummyConstraint = 30,
    Element = 14,
    Circuit = 15,
    Routes = 23,
    Inverse = 18,
    Reservoir = 24,
    Table = 16,
    Automaton = 17,
    Interval = 19,
    NoOverlap = 20,
    NoOverlap2D = 21,
    Cumulative = 22,
    ConstraintNotSet = 0,
}

// The largest possible value of ConstraintProto::constraint_case.
pub const K_LARGEST_CONSTRAINT_TYPE: ConstraintCase = ConstraintCase::DummyConstraint;

// kLargestConstraintType should be less than 2^kConstraintTypeBitSize.
pub const K_CONSTRAINT_TYPE_BIT_SIZE: i32 = 5;
const _: () = assert!((K_LARGEST_CONSTRAINT_TYPE as i32) < (1 << K_CONSTRAINT_TYPE_BIT_SIZE));

// The generated `constraint_case()` of C++: the kind of the constraint that is set.
pub fn constraint_case(ct: &ConstraintProto) -> ConstraintCase {
    match &ct.constraint {
        None => ConstraintCase::ConstraintNotSet,
        Some(Constraint::BoolOr(_)) => ConstraintCase::BoolOr,
        Some(Constraint::BoolAnd(_)) => ConstraintCase::BoolAnd,
        Some(Constraint::AtMostOne(_)) => ConstraintCase::AtMostOne,
        Some(Constraint::ExactlyOne(_)) => ConstraintCase::ExactlyOne,
        Some(Constraint::BoolXor(_)) => ConstraintCase::BoolXor,
        Some(Constraint::IntDiv(_)) => ConstraintCase::IntDiv,
        Some(Constraint::IntMod(_)) => ConstraintCase::IntMod,
        Some(Constraint::LinMax(_)) => ConstraintCase::LinMax,
        Some(Constraint::IntProd(_)) => ConstraintCase::IntProd,
        Some(Constraint::Linear(_)) => ConstraintCase::Linear,
        Some(Constraint::AllDiff(_)) => ConstraintCase::AllDiff,
        Some(Constraint::DummyConstraint(_)) => ConstraintCase::DummyConstraint,
        Some(Constraint::Element(_)) => ConstraintCase::Element,
        Some(Constraint::Circuit(_)) => ConstraintCase::Circuit,
        Some(Constraint::Routes(_)) => ConstraintCase::Routes,
        Some(Constraint::Inverse(_)) => ConstraintCase::Inverse,
        Some(Constraint::Reservoir(_)) => ConstraintCase::Reservoir,
        Some(Constraint::Table(_)) => ConstraintCase::Table,
        Some(Constraint::Automaton(_)) => ConstraintCase::Automaton,
        Some(Constraint::Interval(_)) => ConstraintCase::Interval,
        Some(Constraint::NoOverlap(_)) => ConstraintCase::NoOverlap,
        Some(Constraint::NoOverlap2d(_)) => ConstraintCase::NoOverlap2D,
        Some(Constraint::Cumulative(_)) => ConstraintCase::Cumulative,
    }
}

pub const fn constraint_case_name(constraint_case: ConstraintCase) -> &'static str {
    match constraint_case {
        ConstraintCase::BoolOr => "kBoolOr",
        ConstraintCase::BoolAnd => "kBoolAnd",
        ConstraintCase::AtMostOne => "kAtMostOne",
        ConstraintCase::ExactlyOne => "kExactlyOne",
        ConstraintCase::BoolXor => "kBoolXor",
        ConstraintCase::IntDiv => "kIntDiv",
        ConstraintCase::IntMod => "kIntMod",
        ConstraintCase::LinMax => "kLinMax",
        ConstraintCase::IntProd => "kIntProd",
        ConstraintCase::Linear => "kLinear",
        ConstraintCase::AllDiff => "kAllDiff",
        ConstraintCase::DummyConstraint => "kDummyConstraint",
        ConstraintCase::Element => "kElement",
        ConstraintCase::Circuit => "kCircuit",
        ConstraintCase::Routes => "kRoutes",
        ConstraintCase::Inverse => "kInverse",
        ConstraintCase::Reservoir => "kReservoir",
        ConstraintCase::Table => "kTable",
        ConstraintCase::Automaton => "kAutomaton",
        ConstraintCase::Interval => "kInterval",
        ConstraintCase::NoOverlap => "kNoOverlap",
        ConstraintCase::NoOverlap2D => "kNoOverlap2D",
        ConstraintCase::Cumulative => "kCumulative",
        ConstraintCase::ConstraintNotSet => "kEmpty",
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use cpsat_proto::FILE_DESCRIPTOR_SET;
    use cpsat_proto::{
        BoolArgumentProto, CpModelProto, LinearArgumentProto, LinearConstraintProto,
        NoOverlapConstraintProto,
    };
    use cpsat_testutil::{expect_death, expect_debug_death, parse_test_proto};
    use cpsat_util::sorted_interval_list::ClosedInterval;
    use prost_reflect::DescriptorPool;

    use super::*;

    // `UnorderedElementsAre` of gMock.
    fn assert_unordered_elements_are(actual: &[i32], expected: &[i32]) {
        let mut actual = actual.to_vec();
        let mut expected = expected.to_vec();
        actual.sort_unstable();
        expected.sort_unstable();
        assert_eq!(actual, expected);
    }

    #[test]
    fn linear_expression_gcd_test_empty() {
        // Arrange
        let expr = LinearExpressionProto::default();

        // Act and Assert
        assert_eq!(linear_expression_gcd(&expr, 0), 0);
    }

    #[test]
    fn linear_expression_gcd_test_basic_cases1() {
        // Arrange
        let expr = LinearExpressionProto {
            offset: 2,
            coeffs: vec![4],
            ..Default::default()
        };

        // Act and Assert
        assert_eq!(linear_expression_gcd(&expr, 0), 2);
    }

    #[test]
    fn linear_expression_gcd_test_basic_cases2() {
        // Arrange
        let expr = LinearExpressionProto {
            offset: 5,
            coeffs: vec![10],
            ..Default::default()
        };

        // Act and Assert
        assert_eq!(linear_expression_gcd(&expr, 0), 5);
    }

    #[test]
    fn linear_expression_gcd_test_basic_cases3() {
        // Arrange
        let expr = LinearExpressionProto {
            offset: 9,
            coeffs: vec![12, 24],
            ..Default::default()
        };

        // Act and Assert
        assert_eq!(linear_expression_gcd(&expr, 0), 3);
    }

    #[test]
    fn add_references_used_by_constraint_test_literals() {
        // Arrange
        let mut ct = ConstraintProto::default();
        ct.enforcement_literal.push(1);
        let arg = BoolArgumentProto {
            literals: vec![-10, 10, -20],
        };
        ct.constraint = Some(Constraint::BoolAnd(arg));

        // Act
        let references = get_references_used_by_constraint(&ct);

        // Assert
        assert_unordered_elements_are(&references.literals, &[-10, 10, -20]);
    }

    #[test]
    fn add_references_used_by_constraint_test_integer_constraint() {
        // Arrange
        let mut ct = ConstraintProto::default();
        let mut arg = LinearArgumentProto::default();
        let target = arg.target.get_or_insert_default();
        target.vars.push(7);
        target.coeffs.push(2);
        arg.exprs.push(LinearExpressionProto {
            vars: vec![8],
            coeffs: vec![2],
            offset: 0,
        });
        arg.exprs.push(LinearExpressionProto {
            vars: vec![8],
            coeffs: vec![3],
            offset: 0,
        });
        arg.exprs.push(LinearExpressionProto {
            vars: vec![10],
            coeffs: vec![-2],
            offset: 0,
        });
        ct.constraint = Some(Constraint::IntProd(arg));

        // Act
        let references = get_references_used_by_constraint(&ct);

        // Assert
        assert_unordered_elements_are(&references.variables, &[7, 8, 8, 10]);
    }

    #[test]
    fn add_references_used_by_constraint_test_linear_max_constraint() {
        // Arrange
        let mut ct = ConstraintProto::default();
        let mut arg = LinearArgumentProto::default();
        arg.target.get_or_insert_default().vars.push(7);
        arg.exprs.push(LinearExpressionProto::default());
        arg.exprs.push(LinearExpressionProto::default());
        arg.exprs[0].vars.push(8);
        arg.exprs[0].vars.push(9);
        arg.exprs[1].vars.push(9);
        arg.exprs[1].vars.push(10);
        ct.constraint = Some(Constraint::LinMax(arg));

        // Act
        let references = get_references_used_by_constraint(&ct);

        // Assert
        assert_unordered_elements_are(&references.variables, &[7, 8, 9, 9, 10]);
    }

    #[test]
    fn add_references_used_by_constraint_test_linear_constraint() {
        // Arrange
        let mut ct = ConstraintProto::default();
        let arg = LinearConstraintProto {
            vars: vec![0, 1, 2],
            ..Default::default()
        };
        ct.constraint = Some(Constraint::Linear(arg));

        // Act
        let references = get_references_used_by_constraint(&ct);

        // Assert
        assert_unordered_elements_are(&references.variables, &[0, 1, 2]);
    }

    #[test]
    fn used_interval_test_intervals() {
        // Arrange
        let mut ct = ConstraintProto::default();
        let arg = NoOverlapConstraintProto {
            intervals: vec![0, 1, 2],
        };
        ct.constraint = Some(Constraint::NoOverlap(arg));

        // Act and Assert
        assert_unordered_elements_are(&used_intervals(&ct), &[0, 1, 2]);
    }

    #[test]
    fn used_variables_test_basic_test() {
        // Arrange
        let mut ct = ConstraintProto::default();
        ct.enforcement_literal.push(negated_ref(7));
        let arg = LinearConstraintProto {
            vars: vec![0, 1, negated_ref(2)],
            ..Default::default()
        };
        ct.constraint = Some(Constraint::Linear(arg));

        // Act and Assert
        assert_eq!(used_variables(&ct), [0, 1, 2, 7]);
    }

    #[test]
    fn used_variables_test_constraint_with_multiple_enforcement() {
        // Arrange
        let mut ct = ConstraintProto::default();
        ct.enforcement_literal.push(negated_ref(7));
        ct.enforcement_literal.push(negated_ref(18));
        let arg = LinearConstraintProto {
            vars: vec![0, 1, negated_ref(2)],
            ..Default::default()
        };
        ct.constraint = Some(Constraint::Linear(arg));

        // Act and Assert
        assert_eq!(used_variables(&ct), [0, 1, 2, 7, 18]);
    }

    #[test]
    fn set_to_negated_linear_expression_test_basic_test() {
        // Arrange
        let expr = LinearExpressionProto {
            offset: 5,
            vars: vec![1, -1],
            coeffs: vec![2, -3],
        };

        // Act
        let mut negated_expr = LinearExpressionProto::default();
        set_to_negated_linear_expression(&expr, &mut negated_expr);

        // Assert
        assert_eq!(negated_expr.vars, [-2, 0]);
        assert_eq!(negated_expr.coeffs, [2, -3]);
        assert_eq!(-5, negated_expr.offset);

        // Arrange
        let expr2 = LinearExpressionProto {
            offset: 3,
            vars: vec![2, -2],
            coeffs: vec![3, -4],
        };

        // Act
        set_to_negated_linear_expression(&expr2, &mut negated_expr);

        // Assert
        assert_eq!(negated_expr.vars, [-3, 1]);
        assert_eq!(negated_expr.coeffs, [3, -4]);
        assert_eq!(-3, negated_expr.offset);
    }

    #[test]
    fn constraint_case_name_test_test_few_cases() {
        // Act and Assert
        assert_eq!("kBoolOr", constraint_case_name(ConstraintCase::BoolOr));
        assert_eq!("kLinear", constraint_case_name(ConstraintCase::Linear));
        assert_eq!(
            "kEmpty",
            constraint_case_name(ConstraintCase::ConstraintNotSet)
        );
    }
    #[test]
    fn compute_inner_objective_test_simple_example() {
        // Arrange
        let model_proto: CpModelProto = parse_test_proto(
            r#"
            variables { domain: [ 0, 5 ] }
            variables { domain: [ 0, 5 ] }
            objective {
              vars: [ 0, 1 ]
              coeffs: [ 2, 3 ]
              offset: 3
              scaling_factor: 1.5
            }
            "#,
        );
        let objective = model_proto.objective.as_ref().unwrap();
        let solution: Vec<i64> = vec![2, 3];

        // Act and Assert
        assert_eq!(compute_inner_objective(objective, &solution), 13);
        assert_eq!(scale_objective_value(objective, 13), (13.0 + 3.0) * 1.5);
        assert_eq!(unscale_objective_value(objective, (13.0 + 3.0) * 1.5), 13.0);
    }

    #[test]
    fn get_ref_from_expression_test_basic_api() {
        // Arrange
        let mut expr = LinearExpressionProto::default();

        // Act and Assert
        assert!(!expression_contains_single_ref(&expr));
        expr.offset = 1;
        assert!(!expression_contains_single_ref(&expr));
        expr.offset = 0;
        expr.vars.push(2);
        expr.coeffs.push(1);
        assert!(expression_contains_single_ref(&expr));
        assert_eq!(2, get_single_ref_from_expression(&expr));
        expr.coeffs[0] = -1;
        assert!(expression_contains_single_ref(&expr));
        assert_eq!(-3, get_single_ref_from_expression(&expr));
        expr.coeffs[0] = 2;
        assert!(!expression_contains_single_ref(&expr));
        expr.coeffs[0] = 1;
        expr.vars.push(3);
        expr.coeffs.push(-1);
        assert!(!expression_contains_single_ref(&expr));
    }

    #[test]
    fn add_linear_expression_to_linear_constraint_test_basic_api() {
        // Arrange
        let mut linear = LinearConstraintProto {
            vars: vec![1],
            coeffs: vec![-1],
            domain: vec![2, 4],
        };
        let expr = LinearExpressionProto {
            vars: vec![2, 3],
            coeffs: vec![2, 5],
            offset: 1,
        };

        // Act
        add_linear_expression_to_linear_constraint(&expr, -7, &mut linear);

        // Assert
        let expected_linear_constraint: LinearConstraintProto = parse_test_proto(
            r#"
            vars: [ 1, 2, 3 ]
            coeffs: [ -1, -14, -35 ]
            domain: [ 9, 11 ]
            "#,
        );
        assert_eq!(linear, expected_linear_constraint);
    }

    #[test]
    fn add_weighted_literal_to_linear_constraint_test_basic_api() {
        // Arrange
        let mut linear = LinearConstraintProto {
            vars: vec![1],
            coeffs: vec![-1],
            domain: vec![2, 4],
        };
        let mut offset: i64 = 0;

        // Act
        add_weighted_literal_to_linear_constraint(0, 4, &mut linear, &mut offset);

        // Assert
        let expected_linear_constraint_1: LinearConstraintProto = parse_test_proto(
            r#"
            vars: [ 1, 0 ]
            coeffs: [ -1, 4 ]
            domain: [ 2, 4 ]
            "#,
        );
        assert_eq!(linear, expected_linear_constraint_1);
        assert_eq!(offset, 0);

        // Act
        add_weighted_literal_to_linear_constraint(-3, 5, &mut linear, &mut offset);

        // Assert
        let expected_linear_constraint_2: LinearConstraintProto = parse_test_proto(
            r#"
            vars: [ 1, 0, 2 ]
            coeffs: [ -1, 4, -5 ]
            domain: [ 2, 4 ]
            "#,
        );
        assert_eq!(linear, expected_linear_constraint_2);
        assert_eq!(offset, 5);
    }

    #[test]
    fn safe_add_linear_expression_to_linear_constraint_test_basic_api() {
        // Arrange
        let mut linear = LinearConstraintProto {
            vars: vec![1],
            coeffs: vec![-1],
            domain: vec![2, 4],
        };
        let expr = LinearExpressionProto {
            vars: vec![2, 3],
            coeffs: vec![2, 5],
            offset: 1,
        };
        let large_coeff = LinearExpressionProto {
            vars: vec![0],
            coeffs: vec![10_000_000_000],
            offset: 0,
        };
        let large_offset = LinearExpressionProto {
            offset: 10_000_000_000,
            ..Default::default()
        };

        // Act and Assert
        assert!(safe_add_linear_expression_to_linear_constraint(
            &expr,
            -7,
            &mut linear
        ));
        assert!(!safe_add_linear_expression_to_linear_constraint(
            &large_coeff,
            10_000_000_000,
            &mut linear
        ));
        assert!(!safe_add_linear_expression_to_linear_constraint(
            &large_offset,
            10_000_000_000,
            &mut linear
        ));
    }
    // Fingerprint must be stable. So we can use golden values.
    #[test]
    fn fingerprint_test_basic_api() {
        // Arrange
        let lin: LinearExpressionProto = parse_test_proto(
            r#"
            vars: [ 1, 2, 3 ]
            coeffs: [ -1, -14, -35 ]
            offset: 11
            "#,
        );

        // Act and Assert
        assert_eq!(
            0x871AE5CE74BFBE37_u64,
            fingerprint_expression(&lin, K_DEFAULT_FINGERPRINT_SEED)
        );
        assert_eq!(
            0x3E7E7DEAEF2AB62C_u64,
            fingerprint_repeated_field(&lin.vars, K_DEFAULT_FINGERPRINT_SEED)
        );
        assert_eq!(
            0x85715ADBDFD6F8AD_u64,
            fingerprint_single_field(&lin.offset, K_DEFAULT_FINGERPRINT_SEED)
        );
    }

    #[test]
    fn convert_cp_model_proto_to_cnf_test_basic_example() {
        // Arrange
        let model_proto: CpModelProto = parse_test_proto(
            r#"
            variables { domain: [ 0, 1 ] }
            variables { domain: [ 0, 1 ] }
            variables { domain: [ 0, 1 ] }
            constraints { bool_or { literals: [ 0, 1, 2 ] } }
            constraints { bool_or { literals: [ -1, -2 ] } }
            constraints {
              enforcement_literal: [ 2 ]
              bool_or { literals: [ -1, -2 ] }
            }
            constraints {
              enforcement_literal: [ 0, 1 ]
              bool_and { literals: [ -1, -2 ] }
            }
            "#,
        );
        let expected = "p cnf 3 5
1 2 3 0
-1 -2 0
-3 -1 -2 0
-1 -2 -1 0
-1 -2 -2 0
";
        let mut cnf = String::new();

        // Act
        let converted = convert_cp_model_proto_to_cnf(&model_proto, &mut cnf);

        // Assert
        assert!(converted);
        assert_eq!(expected, cnf);
    }

    #[test]
    fn convert_cp_model_proto_to_cnf_test_fixed_variables() {
        // Arrange
        let model_proto: CpModelProto = parse_test_proto(
            r#"
            variables { domain: [ 0, 1 ] }
            variables { domain: [ 0, 0 ] }
            variables { domain: [ 1, 1 ] }
            constraints { bool_or { literals: [ 0, 1, 2 ] } }
            constraints { bool_or { literals: [ -1, -2 ] } }
            constraints { bool_or { literals: [ -1, -2 ] } }
            "#,
        );
        let expected = "p cnf 3 5
-2 0
3 0
1 2 3 0
-1 -2 0
-1 -2 0
";
        let mut cnf = String::new();

        // Act
        let converted = convert_cp_model_proto_to_cnf(&model_proto, &mut cnf);

        // Assert
        assert!(converted);
        assert_eq!(expected, cnf);
    }

    #[test]
    fn convert_cp_model_proto_to_cnf_test_bool_and() {
        // Arrange
        let model_proto: CpModelProto = parse_test_proto(
            r#"
            variables { domain: [ 0, 1 ] }
            variables { domain: [ 0, 1 ] }
            variables { domain: [ 0, 1 ] }
            constraints { bool_and { literals: [ 0, 1, 2 ] } }
            "#,
        );
        let expected = "p cnf 3 3
1 0
2 0
3 0
";
        let mut cnf = String::new();

        // Act
        let converted = convert_cp_model_proto_to_cnf(&model_proto, &mut cnf);

        // Assert
        assert!(converted);
        assert_eq!(expected, cnf);
    }

    #[test]
    fn largest_constraint_type_test_is_correct() {
        // Arrange
        let pool = DescriptorPool::decode(FILE_DESCRIPTOR_SET).expect("the descriptor set decodes");
        let message_descriptor = pool
            .get_message_by_name("operations_research.sat.ConstraintProto")
            .expect("ConstraintProto is in the descriptor set");

        let oneof_descriptor = message_descriptor
            .oneofs()
            .find(|oneof| oneof.name() == "constraint")
            .expect("Oneof 'constraint' not found");

        // Act
        let mut max_field_number: i32 = -1;
        for field in oneof_descriptor.fields() {
            max_field_number = max_field_number.max(field.number() as i32);
        }

        // Assert
        assert_eq!(
            max_field_number, K_LARGEST_CONSTRAINT_TYPE as i32,
            "kLargestConstraintType ({}) does not match the largest field number in the \
             'constraint' oneof ({}). Please update the constant.",
            K_LARGEST_CONSTRAINT_TYPE as i32, max_field_number
        );

        assert!(
            (K_LARGEST_CONSTRAINT_TYPE as i32) < 1 << K_CONSTRAINT_TYPE_BIT_SIZE,
            "kLargestConstraintType ({}) is not less than 2^kConstraintTypeBitSize ({}). \
             Please update kConstraintTypeBitSize.",
            K_LARGEST_CONSTRAINT_TYPE as i32,
            1 << K_CONSTRAINT_TYPE_BIT_SIZE
        );
    }

    #[test]
    fn affine_expr_test_to_string_and_stringify() {
        // Arrange
        let expr = |var, coeff, offset| AffineExpr { var, coeff, offset };

        // Act and Assert
        assert_eq!(expr(-1, 0, 42).to_string(), "42");
        assert_eq!(expr(5, 0, 12).to_string(), "12");
        assert_eq!(expr(3, 1, 0).to_string(), "X3");
        assert_eq!(expr(3, 1, 5).to_string(), "X3 + 5");
        assert_eq!(expr(3, 1, -5).to_string(), "X3 - 5");
        assert_eq!(expr(3, -1, 0).to_string(), "-X3");
        assert_eq!(expr(3, -1, 5).to_string(), "-X3 + 5");
        assert_eq!(expr(3, -1, -5).to_string(), "-X3 - 5");
        assert_eq!(expr(2, 4, 0).to_string(), "4 * X2");
        assert_eq!(expr(2, 4, 10).to_string(), "4 * X2 + 10");
        assert_eq!(expr(2, 4, -10).to_string(), "4 * X2 - 10");
        assert_eq!(format!("{}", expr(1, 2, 3)), "2 * X1 + 3");
    }

    #[test]
    fn affine_expr_test_equality_and_hash() {
        // Arrange
        let a = AffineExpr {
            var: 1,
            coeff: 2,
            offset: 3,
        };
        let b = AffineExpr {
            var: 1,
            coeff: 2,
            offset: 3,
        };
        let c = AffineExpr {
            var: 1,
            coeff: 2,
            offset: 4,
        };
        let d = AffineExpr {
            var: 2,
            coeff: 2,
            offset: 3,
        };
        let e = AffineExpr {
            var: 1,
            coeff: 1,
            offset: 3,
        };

        // Act
        let mut expr_set: HashSet<AffineExpr> = HashSet::new();
        expr_set.insert(a);

        // Assert
        assert_eq!(a, b);
        assert_ne!(a, c);
        assert_ne!(a, d);
        assert_ne!(a, e);
        assert!(expr_set.contains(&b));
        assert!(!expr_set.contains(&c));
    }

    #[test]
    fn affine_expr_test_min_and_max_evaluation() {
        // Arrange
        let model_proto: CpModelProto = parse_test_proto(
            r#"
            variables { domain: [ 2, 10 ] }
            variables { domain: [ -5, 3 ] }
            variables {}
            "#,
        );

        // Constant expression
        let constant_expr = AffineExpr {
            var: -1,
            coeff: 0,
            offset: 7,
        };
        assert_eq!(get_affine_expr_min(&constant_expr, &model_proto), 7);
        assert_eq!(get_affine_expr_max(&constant_expr, &model_proto), 7);

        // Positive coefficient: var 0 in [2, 10], 3 * X0 + 4 -> [10, 34]
        let pos_expr = AffineExpr {
            var: 0,
            coeff: 3,
            offset: 4,
        };
        assert_eq!(get_affine_expr_min(&pos_expr, &model_proto), 10);
        assert_eq!(get_affine_expr_max(&pos_expr, &model_proto), 34);

        // Negative coefficient: var 1 in [-5, 3], -2 * X1 + 1 -> [-5, 11]
        let neg_expr = AffineExpr {
            var: 1,
            coeff: -2,
            offset: 1,
        };
        assert_eq!(get_affine_expr_min(&neg_expr, &model_proto), -5);
        assert_eq!(get_affine_expr_max(&neg_expr, &model_proto), 11);
    }

    #[test]
    fn compute_float_objective_value_test_directed_rounding_for_min_and_max() {
        // 1.0 + 2^-53 is halfway between 1.0 and nextafter(1.0, inf) and rounds to
        // nearest-even (1.0) in standard double arithmetic.
        // Arrange
        let mut proto = FloatObjectiveProto {
            vars: vec![0, 1],
            coeffs: vec![1.0, 2.0_f64.powi(-53)],
            ..Default::default()
        };
        let solution: Vec<i64> = vec![1, 1];

        // Act
        proto.maximize = false;
        let minimized = compute_float_objective_value(&proto, &solution);
        proto.maximize = true;
        let maximized = compute_float_objective_value(&proto, &solution);

        // Assert
        assert_eq!(minimized, 1.0_f64.next_up());
        assert_eq!(maximized, 1.0);
    }

    #[test]
    fn compute_float_objective_value_test_directed_rounding_for_zero_point_one_plus_two() {
        // In infinite precision, double(0.1) + double(0.2) lies strictly between
        // nextafter(0.1 + 0.2, -inf) and (0.1 + 0.2).
        // Arrange
        let mut proto = FloatObjectiveProto {
            vars: vec![0, 1],
            coeffs: vec![0.1, 0.2],
            ..Default::default()
        };
        let solution: Vec<i64> = vec![1, 1];

        let rounded_up = 0.1 + 0.2;
        let rounded_down = f64::next_down(rounded_up);

        // Act
        proto.maximize = false;
        let minimized = compute_float_objective_value(&proto, &solution);
        proto.maximize = true;
        let maximized = compute_float_objective_value(&proto, &solution);

        // Assert
        assert_eq!(minimized, rounded_up);
        assert_eq!(maximized, rounded_down);
    }

    #[test]
    fn compute_float_objective_value_test_exact_with_large_int64_solution() {
        // (2^53 + 1) cannot be represented in double without losing the +1 bit.
        // Arrange
        let mut proto = FloatObjectiveProto {
            vars: vec![0, 1],
            coeffs: vec![1.0, 1.0],
            ..Default::default()
        };
        let solution: Vec<i64> = vec![(1_i64 << 53) + 1, -(1_i64 << 53)];

        // Act
        proto.maximize = false;
        let minimized = compute_float_objective_value(&proto, &solution);
        proto.maximize = true;
        let maximized = compute_float_objective_value(&proto, &solution);

        // Assert
        assert_eq!(minimized, 1.0);
        assert_eq!(maximized, 1.0);
    }
    #[test]
    fn canonicalize_lin_max_test_sorts_terms_and_expressions() {
        // Arrange
        let mut ct: ConstraintProto = parse_test_proto(
            r#"
            lin_max {
              target {
                vars: [ 2, 1, 2 ]
                coeffs: [ 1, 3, 2 ]
                offset: 4
              }
              exprs {
                vars: [ 1, 0 ]
                coeffs: [ 2, 1 ]
                offset: 5
              }
              exprs {
                vars: [ 0, 1 ]
                coeffs: [ 1, -1 ]
                offset: 10
              }
              exprs {
                vars: [ 0, 1 ]
                coeffs: [ 1, -1 ]
                offset: 2
              }
            }
            "#,
        );
        assert!(!lin_max_is_canonical(&ct));

        // Act
        canonicalize_lin_max(&mut ct);

        // Assert
        assert!(lin_max_is_canonical(&ct));
        let expected: ConstraintProto = parse_test_proto(
            r#"
            lin_max {
              target {
                vars: [ 1, 2 ]
                coeffs: [ 3, 3 ]
                offset: 4
              }
              exprs {
                vars: [ 0, 1 ]
                coeffs: [ 1, -1 ]
                offset: 2
              }
              exprs {
                vars: [ 0, 1 ]
                coeffs: [ 1, -1 ]
                offset: 10
              }
              exprs {
                vars: [ 0, 1 ]
                coeffs: [ 1, 2 ]
                offset: 5
              }
            }
            "#,
        );
        assert_eq!(ct, expected);
    }

    #[test]
    fn canonicalize_lin_max_test_cancels_zero_coefficient_terms() {
        // Arrange
        let mut ct: ConstraintProto = parse_test_proto(
            r#"
            lin_max {
              target {
                vars: [ 0, 1, 0 ]
                coeffs: [ 2, 0, -2 ]
                offset: 3
              }
              exprs {
                vars: [ 1, 1 ]
                coeffs: [ 4, -4 ]
                offset: 7
              }
              exprs { offset: 1 }
            }
            "#,
        );
        assert!(!lin_max_is_canonical(&ct));

        // Act
        canonicalize_lin_max(&mut ct);

        // Assert
        assert!(lin_max_is_canonical(&ct));
        let expected: ConstraintProto = parse_test_proto(
            r#"
            lin_max {
              target { offset: 3 }
              exprs { offset: 1 }
              exprs { offset: 7 }
            }
            "#,
        );
        assert_eq!(ct, expected);
    }

    #[test]
    fn canonicalize_lin_max_test_canonicalizes_enforcement_literals() {
        // Arrange
        let mut ct: ConstraintProto = parse_test_proto(
            r#"
            enforcement_literal: [ 1, -1, 1 ]
            lin_max {
              target {
                vars: [ 0, 1, 2 ]
                coeffs: [ 3, 4, 5 ]
                offset: 1
              }
              exprs {
                vars: [ 1, 2 ]
                coeffs: [ 2, 1 ]
                offset: 10
              }
              exprs {
                vars: [ 0, 2 ]
                coeffs: [ -2, 1 ]
                offset: 5
              }
            }
            "#,
        );
        assert!(!lin_max_is_canonical(&ct));

        // Act
        canonicalize_lin_max(&mut ct);

        // Assert
        assert!(lin_max_is_canonical(&ct));
        let expected: ConstraintProto = parse_test_proto(
            r#"
            enforcement_literal: [ -1, 1 ]
            lin_max {
              target {
                vars: [ 2 ]
                coeffs: [ 5 ]
                offset: 5
              }
              exprs {
                vars: [ 2 ]
                coeffs: [ 1 ]
                offset: 5
              }
              exprs {
                vars: [ 2 ]
                coeffs: [ 1 ]
                offset: 12
              }
            }
            "#,
        );
        assert_eq!(ct, expected);

        // Contradictory enforcement literals (x and not(x)) are not canonical and
        // clear the constraint when canonicalized.
        // Arrange
        let mut contradictory_ct: ConstraintProto = parse_test_proto(
            r#"
            enforcement_literal: [ -1, 0 ]
            lin_max {
              target {
                vars: [ 2 ]
                coeffs: [ 1 ]
              }
              exprs {
                vars: [ 2 ]
                coeffs: [ 1 ]
              }
            }
            "#,
        );
        assert!(!lin_max_is_canonical(&contradictory_ct));

        // Act
        canonicalize_lin_max(&mut contradictory_ct);

        // Assert
        assert!(contradictory_ct.constraint.is_none());
    }

    #[test]
    fn affine_expr_test_const_and_var_helpers() {
        // Arrange
        const C: AffineExpr = AffineExpr::const_(42);
        const V: AffineExpr = AffineExpr::var(7);
        let scaled = AffineExpr {
            var: 5,
            coeff: 0,
            offset: 10,
        };

        // Act and Assert
        assert_eq!(C.var, -1);
        assert_eq!(C.coeff, 0);
        assert_eq!(C.offset, 42);
        assert!(C.is_constant());
        assert_eq!(C.constant_value(), 42);

        assert_eq!(V.var, 7);
        assert_eq!(V.coeff, 1);
        assert_eq!(V.offset, 0);
        assert!(!V.is_constant());

        assert!(scaled.is_constant());
        assert_eq!(scaled.constant_value(), 10);
    }

    #[test]
    fn cp_model_utils_test_apply_to_all_linear_expressions() {
        // Unperformed optional interval stub: mutable visitor must not materialize
        // start/size/end submessages.
        // Arrange
        let mut unperformed_interval: ConstraintProto = parse_test_proto(
            r#"
            enforcement_literal: [ 0 ]
            interval {}
            "#,
        );
        let mut visited = 0;

        // Act
        apply_to_all_linear_expressions(
            |_expr: &mut LinearExpressionProto| visited += 1,
            &mut unperformed_interval,
        );

        // Assert
        assert_eq!(visited, 0);
        let Some(Constraint::Interval(interval)) = &unperformed_interval.constraint else {
            panic!("the constraint is an interval");
        };
        assert!(interval.start.is_none());
        assert!(interval.size.is_none());
        assert!(interval.end.is_none());

        // Routes node expressions are visited by both mutable and const overloads.
        // Arrange
        let mut routes_ct: ConstraintProto = parse_test_proto(
            r#"
            routes {
              tails: [ 0 ]
              heads: [ 1 ]
              literals: [ 0 ]
              dimensions {
                exprs {
                  vars: [ 1 ]
                  coeffs: [ 2 ]
                  offset: 3
                }
              }
            }
            "#,
        );
        let mut routes_visited = 0;

        // Act
        apply_to_all_linear_expressions(
            |expr: &mut LinearExpressionProto| {
                expr.offset += 10;
                routes_visited += 1;
            },
            &mut routes_ct,
        );

        // Assert
        assert_eq!(routes_visited, 1);
        let Some(Constraint::Routes(routes)) = &routes_ct.constraint else {
            panic!("the constraint is a routes constraint");
        };
        assert_eq!(routes.dimensions[0].exprs[0].offset, 13);

        // Arrange
        let const_routes: &ConstraintProto = &routes_ct;
        let mut const_visited = 0;

        // Act
        apply_to_all_linear_expressions_const(
            |expr: &LinearExpressionProto| {
                assert_eq!(expr.offset, 13);
                const_visited += 1;
            },
            const_routes,
        );

        // Assert
        assert_eq!(const_visited, 1);

        // Legacy table with vars populated should not visit exprs.
        // Arrange
        let legacy_table: ConstraintProto = parse_test_proto(
            r#"
            table {
              vars: [ 0, 1 ]
              values: [ 0, 1 ]
            }
            "#,
        );
        visited = 0;

        // Act
        apply_to_all_linear_expressions_const(
            |_expr: &LinearExpressionProto| visited += 1,
            &legacy_table,
        );

        // Assert
        assert_eq!(visited, 0);
    }

    #[test]
    fn cp_model_utils_test_canonicalize_linear_constraint_proto() {
        // Arrange
        let mut lin: LinearConstraintProto = parse_test_proto(
            r#"
            vars: [ 2, 0, 2, 1 ]
            coeffs: [ 3, 4, -1, 5 ]
            domain: [ 10, 20 ]
            "#,
        );

        // Act
        // Enforcement: var 0 == 1 (shifts domain by -4), var 1 == 0 (dropped).
        canonicalize_linear_expression_in_constraint(&HashSet::from([0, -2]), &mut lin);

        // Assert
        let expected: LinearConstraintProto = parse_test_proto(
            r#"
            vars: [ 2 ]
            coeffs: [ 2 ]
            domain: [ 6, 16 ]
            "#,
        );
        assert_eq!(lin, expected);
    }

    #[test]
    fn cp_model_utils_test_sorted_literal_complementary_helpers() {
        // Act and Assert
        assert_eq!(find_complementary_var_in_sorted_literals(&[]), -1);
        assert_eq!(find_complementary_var_in_sorted_literals(&[-3, -1]), -1);
        assert_eq!(find_complementary_var_in_sorted_literals(&[0, 2]), -1);
        assert_eq!(
            find_complementary_var_in_sorted_literals(&[-4, -2, 0, 2]),
            -1
        );
        assert!(!sorted_literals_have_opposite_pair(&[-4, -2, 0, 2]));

        // NegatedRef(2) == -3, so {-5, -3, 0, 2, 5} has complementary var 2.
        assert_eq!(
            find_complementary_var_in_sorted_literals(&[-5, -3, 0, 2, 5]),
            2
        );
        assert!(sorted_literals_have_opposite_pair(&[-5, -3, 0, 2, 5]));

        assert!(!sorted_literals_have_opposite_across(&[], &[0, 1]));
        assert!(!sorted_literals_have_opposite_across(
            &[-4, -2, 0],
            &[-3, 2, 5]
        ));
        // NegatedRef(-3) == 2 and NegatedRef(2) == -3.
        assert!(sorted_literals_have_opposite_across(
            &[-5, -3, 0],
            &[-2, 2, 4]
        ));
        assert!(sorted_literals_have_opposite_across(
            &[-2, 2, 4],
            &[-5, -3, 0]
        ));
    }

    #[test]
    fn convert_cp_model_proto_to_wcnf_writes_hard_clauses_and_weights() {
        // Arrange
        let model_proto: CpModelProto = parse_test_proto(
            r#"
            variables { domain: [ 0, 1 ] }
            variables { domain: [ 0, 1 ] }
            constraints { bool_or { literals: [ 0, 1 ] } }
            objective {
              vars: [ 0, 1 ]
              coeffs: [ -2, 3 ]
              scaling_factor: -1
            }
            "#,
        );
        let mut wcnf = String::new();

        // Act
        let converted = convert_cp_model_proto_to_wcnf(&model_proto, &mut wcnf);

        // Assert
        assert!(converted);
        assert_eq!("h 1 2 0\n2 1 0\n3 -2 0\n", wcnf);
    }

    #[test]
    fn convert_cp_model_proto_to_wcnf_refuses_a_model_without_an_integral_objective() {
        // Arrange
        let no_objective: CpModelProto = parse_test_proto(
            r#"
            variables { domain: [ 0, 1 ] }
            constraints { bool_or { literals: [ 0 ] } }
            "#,
        );
        let fractional_scaling: CpModelProto = parse_test_proto(
            r#"
            variables { domain: [ 0, 1 ] }
            constraints { bool_or { literals: [ 0 ] } }
            objective { vars: [ 0 ] coeffs: [ 1 ] scaling_factor: 0.5 }
            "#,
        );
        let mut out = String::from("old");

        // Act and Assert
        assert!(!convert_cp_model_proto_to_wcnf(&no_objective, &mut out));
        assert!(out.is_empty());
        assert!(!convert_cp_model_proto_to_wcnf(
            &fractional_scaling,
            &mut out
        ));
    }

    #[test]
    fn convert_cp_model_proto_to_cnf_refuses_a_model_that_is_not_pure_sat() {
        // Arrange
        let wide_domain: CpModelProto = parse_test_proto(
            r#"
            variables { domain: [ 0, 2 ] }
            "#,
        );
        let linear: CpModelProto = parse_test_proto(
            r#"
            variables { domain: [ 0, 1 ] }
            constraints { linear { vars: [ 0 ] coeffs: [ 1 ] domain: [ 0, 1 ] } }
            "#,
        );
        let mut out = String::new();

        // Act and Assert
        assert!(!convert_cp_model_proto_to_cnf(&wide_domain, &mut out));
        assert!(!convert_cp_model_proto_to_cnf(&linear, &mut out));
    }

    #[test]
    fn combine_seed_is_non_negative_and_depends_on_both_arguments() {
        // Act
        let base = combine_seed(7, 3);

        // Assert
        assert!(base >= 0);
        assert_eq!(base, combine_seed(7, 3));
        assert_ne!(base, combine_seed(8, 3));
        assert_ne!(base, combine_seed(7, 4));
    }

    #[test]
    fn combine_seed_of_a_negative_delta_dies() {
        // Act and Assert
        expect_death(
            || {
                combine_seed(7, -1);
            },
            "assertion failed: delta >= 0",
        );
    }

    #[test]
    fn fingerprint_model_is_stable_and_sees_each_part_of_the_model() {
        // Arrange
        let text = r#"
            variables { domain: [ 0, 5 ] }
            variables { domain: [ 0, 5 ] }
            constraints {
              enforcement_literal: [ 0 ]
              linear { vars: [ 0, 1 ] coeffs: [ 1, 2 ] domain: [ 0, 4 ] }
            }
            constraints { lin_max { target { vars: [ 1 ] coeffs: [ 1 ] } } }
            constraints { interval { start { offset: 1 } } }
            objective { vars: [ 0 ] coeffs: [ 1 ] }
            solution_hint { vars: [ 0 ] values: [ 2 ] }
            "#;
        let model: CpModelProto = parse_test_proto(text);
        let mut changed_coeff = model.clone();
        if let Some(Constraint::Linear(linear)) = &mut changed_coeff.constraints[0].constraint {
            linear.coeffs[1] = 3;
        }
        let mut changed_hint = model.clone();
        changed_hint.solution_hint.as_mut().unwrap().values[0] = 3;
        let mut floating = model.clone();
        floating.objective = None;
        floating.floating_point_objective = Some(FloatObjectiveProto {
            vars: vec![0],
            coeffs: vec![1.5],
            ..Default::default()
        });

        // Act
        let fingerprint = fingerprint_model(&model, K_DEFAULT_FINGERPRINT_SEED);

        // Assert
        assert_eq!(
            fingerprint,
            fingerprint_model(&model, K_DEFAULT_FINGERPRINT_SEED)
        );
        assert_ne!(fingerprint, fingerprint_model(&model, 1));
        assert_ne!(
            fingerprint,
            fingerprint_model(&changed_coeff, K_DEFAULT_FINGERPRINT_SEED)
        );
        assert_ne!(
            fingerprint,
            fingerprint_model(&changed_hint, K_DEFAULT_FINGERPRINT_SEED)
        );
        assert_ne!(
            fingerprint,
            fingerprint_model(&floating, K_DEFAULT_FINGERPRINT_SEED)
        );
    }

    #[test]
    fn write_model_proto_to_file_writes_the_binary_form_and_reports_a_failure() {
        // Arrange
        let model: CpModelProto = parse_test_proto(r#"variables { domain: [ 0, 1 ] }"#);
        let path = std::env::temp_dir().join(format!("cp_model_utils_{}.pb", std::process::id()));
        let path = path.to_str().unwrap();

        // Act
        let written = write_model_proto_to_file(&model, path);
        let read_back =
            fs::read(path).map(|bytes| <CpModelProto as prost::Message>::decode(bytes.as_slice()));
        let _ = fs::remove_file(path);

        // Assert
        assert!(written);
        assert_eq!(read_back.unwrap(), Ok(model.clone()));
        assert!(!write_model_proto_to_file(
            &model,
            "/no/such/folder/model.pb"
        ));
    }

    #[test]
    fn is_affine_int_abs_accepts_the_absolute_value_form_only() {
        // Arrange
        let abs: ConstraintProto = parse_test_proto(
            r#"
            lin_max {
              target { vars: [ 1 ] coeffs: [ 1 ] }
              exprs { vars: [ 0 ] coeffs: [ 2 ] offset: 1 }
              exprs { vars: [ -1 ] coeffs: [ 2 ] offset: -1 }
            }
            "#,
        );
        let other_offset: ConstraintProto = parse_test_proto(
            r#"
            lin_max {
              exprs { vars: [ 0 ] coeffs: [ 2 ] offset: 1 }
              exprs { vars: [ -1 ] coeffs: [ 2 ] offset: 1 }
            }
            "#,
        );
        let same_sign: ConstraintProto = parse_test_proto(
            r#"
            lin_max {
              exprs { vars: [ 0 ] coeffs: [ 2 ] }
              exprs { vars: [ 0 ] coeffs: [ 2 ] }
            }
            "#,
        );

        // Act and Assert
        assert!(is_affine_int_abs(&abs));
        assert!(!is_affine_int_abs(&other_offset));
        assert!(!is_affine_int_abs(&same_sign));
        assert!(!is_affine_int_abs(&ConstraintProto::default()));
    }

    #[test]
    fn linear_expression_protos_are_equal_ignores_the_order_and_applies_the_scaling() {
        // Arrange
        let a: LinearExpressionProto =
            parse_test_proto("vars: [ 1, 2 ] coeffs: [ 4, 6 ] offset: 2");
        let b: LinearExpressionProto =
            parse_test_proto("vars: [ 2, 1 ] coeffs: [ 3, 2 ] offset: 1");

        // Act and Assert
        assert!(linear_expression_protos_are_equal(&a, &b, 2));
        assert!(!linear_expression_protos_are_equal(&a, &b, 1));
        assert!(!linear_expression_protos_are_exactly_equal(&a, &b));
        assert!(linear_expression_protos_are_exactly_equal(&a, &a.clone()));
        assert!(linear_expression_proto_less_than(&a, &b));
        assert!(!linear_expression_proto_less_than(&b, &a));
        assert!(!linear_expression_proto_less_than(&a, &a));
    }

    #[test]
    fn expressions_contains_only_one_var_needs_a_single_shared_variable() {
        // Arrange
        let one: LinearExpressionProto = parse_test_proto("vars: [ 3 ] coeffs: [ 1 ]");
        let again: LinearExpressionProto = parse_test_proto("vars: [ 3 ] coeffs: [ 2 ]");
        let other: LinearExpressionProto = parse_test_proto("vars: [ 4 ] coeffs: [ 1 ]");
        let constant: LinearExpressionProto = parse_test_proto("offset: 1");

        // Act and Assert
        assert!(expressions_contains_only_one_var([&one, &again, &constant]));
        assert!(!expressions_contains_only_one_var([&one, &other]));
        assert!(!expressions_contains_only_one_var([&constant]));
    }

    #[test]
    fn apply_to_all_indices_visits_the_variables_literals_and_intervals_of_each_kind() {
        // Arrange
        let mut int_div: ConstraintProto = parse_test_proto(
            r#"
            enforcement_literal: [ 5 ]
            int_div {
              target { vars: [ 1 ] coeffs: [ 1 ] }
              exprs { vars: [ 2 ] coeffs: [ 1 ] }
            }
            "#,
        );
        let mut bool_or: ConstraintProto =
            parse_test_proto("enforcement_literal: [ 5 ] bool_or { literals: [ 1, -3 ] }");
        let mut cumulative: ConstraintProto = parse_test_proto(
            r#"
            cumulative {
              capacity { vars: [ 9 ] coeffs: [ 1 ] }
              intervals: [ 4, 6 ]
              demands { vars: [ 8 ] coeffs: [ 1 ] }
            }
            "#,
        );
        let mut seen = Vec::new();

        // Act
        apply_to_all_variable_indices(|r| seen.push(*r), &mut int_div);
        apply_to_all_literal_indices(|r| *r += 100, &mut bool_or);
        apply_to_all_variable_indices(|r| seen.push(*r), &mut cumulative);
        apply_to_all_interval_indices(|r| *r += 10, &mut cumulative);

        // Assert
        assert_eq!(seen, [1, 2, 9, 8]);
        assert_eq!(bool_or.enforcement_literal, [105]);
        assert_eq!(
            bool_or.constraint,
            Some(Constraint::BoolOr(BoolArgumentProto {
                literals: vec![101, 97]
            }))
        );
        assert_eq!(used_intervals(&cumulative), [14, 16]);
    }

    #[test]
    fn interval_variables_are_inserted_into_and_removed_from_a_bitset() {
        // Arrange
        let model_proto: CpModelProto = parse_test_proto(
            r#"
            variables { domain: [ 0, 1 ] }
            constraints {
              enforcement_literal: [ -3 ]
              interval {
                start { vars: [ 0 ] coeffs: [ 1 ] }
                end { vars: [ 1 ] coeffs: [ 1 ] }
              }
            }
            constraints { bool_or { literals: [ 0 ] } }
            "#,
        );
        let mut bitset: Bitset64<i32> = Bitset64::new(8);

        // Act
        insert_variables_from_interval(&model_proto, 0, &mut bitset);
        let inserted: Vec<bool> = (0..4).map(|i| bitset.is_set(i)).collect();
        insert_variables_from_interval(&model_proto, 1, &mut bitset);
        remove_variables_from_interval(&model_proto, 0, &mut bitset);

        // Assert
        assert_eq!(inserted, [true, true, true, false]);
        assert!(!bitset.is_set(0) && !bitset.is_set(1) && !bitset.is_set(2));
    }

    #[test]
    fn domain_helpers_read_write_and_search_a_proto_domain() {
        // Arrange
        let mut variable: IntegerVariableProto = parse_test_proto("domain: [ 1, 3, 7, 7 ]");

        // Act and Assert
        assert!(domain_in_proto_contains(&variable, 2));
        assert!(domain_in_proto_contains(&variable, 7));
        assert!(!domain_in_proto_contains(&variable, 5));
        assert_eq!(all_values_in_domain(&variable), [1, 2, 3, 7]);
        assert_eq!(
            read_domain_from_proto(&variable),
            Domain::from_intervals(&[ClosedInterval::new(1, 3), ClosedInterval::new_value(7),])
        );
        fill_domain_in_proto(&Domain::new(4, 6), &mut variable);
        assert_eq!(variable.domain, [4, 6]);
        fill_domain_in_proto_bounds(1, 2, &mut variable);
        assert_eq!(variable.domain, [1, 2]);
        fill_domain_in_proto_value(9, &mut variable);
        assert_eq!(variable.domain, [9, 9]);
    }

    #[test]
    fn objective_value_helpers_scale_postsolve_and_presolve() {
        // Arrange
        let plain = CpObjectiveProto {
            offset: 2.0,
            ..Default::default()
        };
        let integer = CpObjectiveProto {
            integer_before_offset: 3,
            integer_after_offset: 5,
            integer_scaling_factor: 2,
            ..Default::default()
        };
        let integer_unscaled = CpObjectiveProto {
            integer_before_offset: 3,
            ..Default::default()
        };

        // Act and Assert
        assert_eq!(scale_objective_value(&plain, i64::MAX), f64::INFINITY);
        assert_eq!(scale_objective_value(&plain, i64::MIN), f64::NEG_INFINITY);
        assert_eq!(scale_objective_value(&plain, 4), 6.0);
        assert_eq!(postsolve_inner_objective_value(&integer, 4), 19);
        assert_eq!(presolve_inner_objective_value(&integer, 19), 4);
        assert_eq!(
            postsolve_inner_objective_value(&integer, i64::MAX),
            i64::MAX
        );
        assert_eq!(postsolve_inner_objective_value(&integer_unscaled, 4), 7);
        assert_eq!(presolve_inner_objective_value(&integer_unscaled, 7), 4);
    }

    #[test]
    fn linear_expression_helpers_divide_evaluate_and_convert() {
        // Arrange
        let mut expr: LinearExpressionProto =
            parse_test_proto("vars: [ 2 ] coeffs: [ 6 ] offset: 4");
        let constant: LinearExpressionProto = parse_test_proto("offset: 7");

        // Act
        divide_linear_expression(2, &mut expr);

        // Assert
        assert_eq!(expr.coeffs, [3]);
        assert_eq!(expr.offset, 2);
        assert_eq!(affine_expression_value_at(&expr, 5), 17);
        assert_eq!(get_inner_var_value(&expr, 17), 5);
        assert!(affine_expression_contains_var(&expr, 2));
        assert!(!affine_expression_contains_var(&expr, 3));
        assert!(expression_is_affine(&expr));
        assert_eq!(
            get_affine_expr(&expr),
            AffineExpr {
                var: 2,
                coeff: 3,
                offset: 2
            }
        );
        assert_eq!(get_affine_expr(&constant), AffineExpr::const_(7));
        expect_death(
            || divide_linear_expression(0, &mut LinearExpressionProto::default()),
            "left != right",
        );
    }

    #[test]
    fn literals_to_linear_moves_the_offset_of_a_negated_literal_into_the_bounds() {
        // Arrange
        let mut linear = LinearConstraintProto::default();

        // Act
        literals_to_linear(&[2, -4], 1, 1, &mut linear);

        // Assert
        let expected: LinearConstraintProto =
            parse_test_proto("vars: [ 2, 3 ] coeffs: [ 1, -1 ] domain: [ 0, 0 ]");
        assert_eq!(linear, expected);
    }

    #[test]
    fn find_complementary_var_in_sorted_literals_dies_on_unsorted_literals_in_debug() {
        // Act and Assert
        expect_debug_death(
            || {
                find_complementary_var_in_sorted_literals(&[0, -1]);
            },
            "is_sorted",
        );
    }

    // The expected values of the tests below were read from the reference build at the pinned
    // commit, by calling the upstream functions.
    #[test]
    fn affine_expr_to_string_of_the_smallest_offset_matches_the_reference() {
        // Arrange
        let expr = AffineExpr {
            var: 0,
            coeff: 1,
            offset: i64::MIN,
        };

        // Act
        let text = expr.to_string();

        // Assert
        assert_eq!(text, "X0 - -9223372036854775808");
    }

    #[test]
    fn linear_expression_gcd_of_the_smallest_offset_and_0_matches_the_reference() {
        // Arrange
        let expr: LinearExpressionProto = parse_test_proto("offset: -9223372036854775808");

        // Act
        let gcd = linear_expression_gcd(&expr, 0);

        // Assert
        assert_eq!(gcd, i64::MIN);
    }

    #[test]
    fn linear_expression_gcd_of_the_smallest_coefficient_and_0_matches_the_reference() {
        // Arrange
        let expr: LinearExpressionProto =
            parse_test_proto("vars: [ 0 ] coeffs: [ -9223372036854775808 ]");

        // Act
        let gcd = linear_expression_gcd(&expr, 0);

        // Assert
        assert_eq!(gcd, i64::MIN);
    }

    #[test]
    fn linear_expression_gcd_of_the_smallest_offset_and_6_matches_the_reference() {
        // Arrange
        let expr: LinearExpressionProto = parse_test_proto("offset: -9223372036854775808");

        // Act
        let gcd = linear_expression_gcd(&expr, 6);

        // Assert
        assert_eq!(gcd, 2);
    }

    #[test]
    fn linear_expression_gcd_of_the_smallest_coefficient_and_6_matches_the_reference() {
        // Arrange
        let expr: LinearExpressionProto =
            parse_test_proto("vars: [ 0 ] coeffs: [ -9223372036854775808 ]");

        // Act
        let gcd = linear_expression_gcd(&expr, 6);

        // Assert
        assert_eq!(gcd, 2);
    }

    #[test]
    fn expression_contains_single_ref_of_the_smallest_coefficient_matches_the_reference() {
        // Arrange
        let expr: LinearExpressionProto =
            parse_test_proto("vars: [ 0 ] coeffs: [ -9223372036854775808 ]");

        // Act
        let single = expression_contains_single_ref(&expr);

        // Assert
        assert!(!single);
    }

    // The golden values of the fingerprint tests below come from the reference build at the
    // pinned commit.
    const OTHER_FINGERPRINT_SEED: u64 = 12345;

    #[test]
    fn fingerprint_model_of_a_model_with_a_linear_and_a_lin_max_matches_the_reference() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
            variables { domain: [ 0, 5 ] }
            variables { domain: [ 0, 5 ] }
            constraints {
              enforcement_literal: [ 0 ]
              linear { vars: [ 0, 1 ] coeffs: [ 1, 2 ] domain: [ 0, 4 ] }
            }
            constraints { lin_max { target { vars: [ 1 ] coeffs: [ 1 ] } } }
            constraints { interval { start { offset: 1 } } }
            objective { vars: [ 0 ] coeffs: [ 1 ] }
            solution_hint { vars: [ 0 ] values: [ 2 ] }
            "#,
        );

        // Act
        let with_default_seed = fingerprint_model(&model, K_DEFAULT_FINGERPRINT_SEED);
        let with_other_seed = fingerprint_model(&model, OTHER_FINGERPRINT_SEED);

        // Assert
        assert_eq!(with_default_seed, 0xd19c385708aecc14);
        assert_eq!(with_other_seed, 0xd60d0da42ac3de2b);
    }

    #[test]
    fn fingerprint_model_of_a_model_with_boolean_constraints_matches_the_reference() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
            variables { domain: [ 0, 1 ] }
            variables { domain: [ 0, 1 ] }
            constraints { bool_or { literals: [ 0, -2 ] } }
            constraints { exactly_one { literals: [ 0, 1 ] } }
            "#,
        );

        // Act
        let with_default_seed = fingerprint_model(&model, K_DEFAULT_FINGERPRINT_SEED);
        let with_other_seed = fingerprint_model(&model, OTHER_FINGERPRINT_SEED);

        // Assert
        assert_eq!(with_default_seed, 0x95699c5e98fe9254);
        assert_eq!(with_other_seed, 0xe72e0ccb16f8d074);
    }

    #[test]
    fn fingerprint_model_of_a_model_with_all_diff_and_element_matches_the_reference() {
        // Arrange
        let model: CpModelProto = parse_test_proto(
            r#"
            variables { domain: [ 0, 10 ] }
            variables { domain: [ 0, 10 ] }
            variables { domain: [ 0, 10 ] }
            constraints {
              all_diff {
                exprs { vars: [ 0 ] coeffs: [ 1 ] }
                exprs { vars: [ 1 ] coeffs: [ 1 ] offset: 2 }
              }
            }
            constraints { element { index: 2 target: 1 vars: [ 0, 1 ] } }
            objective { vars: [ 0, 1 ] coeffs: [ 1, -2 ] offset: 3 }
            "#,
        );

        // Act
        let with_default_seed = fingerprint_model(&model, K_DEFAULT_FINGERPRINT_SEED);
        let with_other_seed = fingerprint_model(&model, OTHER_FINGERPRINT_SEED);

        // Assert
        assert_eq!(with_default_seed, 0xbf17458621e7b121);
        assert_eq!(with_other_seed, 0xee119c8cff709409);
    }

    #[test]
    fn fingerprint_expression_of_an_expression_with_terms_matches_the_reference() {
        // Arrange
        let lin: LinearExpressionProto = parse_test_proto("vars: [ 4 ] coeffs: [ 2 ] offset: -7");

        // Act
        let with_default_seed = fingerprint_expression(&lin, K_DEFAULT_FINGERPRINT_SEED);
        let with_other_seed = fingerprint_expression(&lin, OTHER_FINGERPRINT_SEED);

        // Assert
        assert_eq!(with_default_seed, 0x1458692e0487bb9b);
        assert_eq!(with_other_seed, 0xaa041115b6950ce7);
    }

    #[test]
    fn fingerprint_expression_of_a_constant_matches_the_reference() {
        // Arrange
        let lin: LinearExpressionProto = parse_test_proto("offset: 5");

        // Act
        let with_default_seed = fingerprint_expression(&lin, K_DEFAULT_FINGERPRINT_SEED);
        let with_other_seed = fingerprint_expression(&lin, OTHER_FINGERPRINT_SEED);

        // Assert
        assert_eq!(with_default_seed, 0xfdb2603f1232ac38);
        assert_eq!(with_other_seed, 0xbfea9edadd2cc526);
    }

    #[test]
    fn fingerprint_expression_of_an_empty_expression_matches_the_reference() {
        // Arrange
        let lin = LinearExpressionProto::default();

        // Act
        let with_default_seed = fingerprint_expression(&lin, K_DEFAULT_FINGERPRINT_SEED);
        let with_other_seed = fingerprint_expression(&lin, OTHER_FINGERPRINT_SEED);

        // Assert
        assert_eq!(with_default_seed, 0xf4751fa5d4643aaa);
        assert_eq!(with_other_seed, 0xd7f19665bf9462ba);
    }

    #[test]
    fn combine_seed_matches_the_reference() {
        // Arrange
        let cases: [(i32, i64, i32); 9] = [
            (0, 0, 22385994),
            (0, 3, 313865978),
            (0, 1 << 40, 82285505),
            (7, 0, 384266844),
            (7, 3, 132849012),
            (7, 1 << 40, 445946258),
            (-1, 0, 847182962),
            (-1, 3, 600126342),
            (-1, 1 << 40, 1573512566),
        ];

        // Act
        let combined: Vec<i32> = cases
            .iter()
            .map(|&(base, delta, _)| combine_seed(base, delta))
            .collect();

        // Assert
        let expected: Vec<i32> = cases.iter().map(|&(_, _, value)| value).collect();
        assert_eq!(combined, expected);
    }

    #[test]
    fn constraint_case_names_the_kind_of_each_constraint_and_the_unset_case() {
        // Arrange
        let cases = [
            ("bool_or {}", ConstraintCase::BoolOr),
            ("bool_and {}", ConstraintCase::BoolAnd),
            ("at_most_one {}", ConstraintCase::AtMostOne),
            ("exactly_one {}", ConstraintCase::ExactlyOne),
            ("bool_xor {}", ConstraintCase::BoolXor),
            ("int_div {}", ConstraintCase::IntDiv),
            ("int_mod {}", ConstraintCase::IntMod),
            ("lin_max {}", ConstraintCase::LinMax),
            ("int_prod {}", ConstraintCase::IntProd),
            ("linear {}", ConstraintCase::Linear),
            ("all_diff {}", ConstraintCase::AllDiff),
            ("dummy_constraint {}", ConstraintCase::DummyConstraint),
            ("element {}", ConstraintCase::Element),
            ("circuit {}", ConstraintCase::Circuit),
            ("routes {}", ConstraintCase::Routes),
            ("inverse {}", ConstraintCase::Inverse),
            ("reservoir {}", ConstraintCase::Reservoir),
            ("table {}", ConstraintCase::Table),
            ("automaton {}", ConstraintCase::Automaton),
            ("interval {}", ConstraintCase::Interval),
            ("no_overlap {}", ConstraintCase::NoOverlap),
            ("no_overlap_2d {}", ConstraintCase::NoOverlap2D),
            ("cumulative {}", ConstraintCase::Cumulative),
            ("", ConstraintCase::ConstraintNotSet),
        ];

        // Act
        let found: Vec<ConstraintCase> = cases
            .iter()
            .map(|(text, _)| constraint_case(&parse_test_proto::<ConstraintProto>(text)))
            .collect();

        // Assert
        let expected: Vec<ConstraintCase> = cases.iter().map(|&(_, case)| case).collect();
        assert_eq!(found, expected);
    }

    #[test]
    fn canonicalize_lin_max_of_another_constraint_dies_in_debug_and_switches_in_release() {
        // Arrange
        let make = || -> ConstraintProto {
            parse_test_proto("enforcement_literal: [ 2, 1, 2 ] bool_or { literals: [ 0 ] }")
        };

        // Act and Assert
        expect_debug_death(
            || canonicalize_lin_max(&mut make()),
            "matches!(ct.constraint, Some(Constraint::LinMax(_)))",
        );
        if !cfg!(debug_assertions) {
            let mut ct = make();
            canonicalize_lin_max(&mut ct);
            let expected: ConstraintProto =
                parse_test_proto("enforcement_literal: [ 1, 2 ] lin_max { target {} }");
            assert_eq!(ct, expected);
        }
    }
}
