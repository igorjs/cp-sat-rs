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

use std::fmt;
use std::sync::atomic::AtomicBool;
use std::sync::{LazyLock, Mutex};

use cpsat_proto::constraint_proto::Constraint;
use cpsat_proto::{ConstraintProto, CpModelProto, LinearExpressionProto};
use cpsat_util::bitset::Bitset64;

// DEBUG ONLY. When set to true, SolveCpModel() will dump its model protos
// (original model, presolved model, mapping model) in text format to
// 'FLAGS_cp_model_dump_prefix'{model|presolved_model|mapping_model}.pb.txt.
pub static CP_MODEL_DUMP_MODELS: AtomicBool = AtomicBool::new(false);

// Prefix filename for all dumped files.
pub static CP_MODEL_DUMP_PREFIX: LazyLock<Mutex<String>> =
    LazyLock::new(|| Mutex::new(if cfg!(windows) { ".\\" } else { "/tmp/" }.to_owned()));

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
        // Negation wraps for the smallest offset, as the optimised C++ build does.
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
    let mut gcd = std_gcd(gcd, expr.offset.wrapping_abs());
    for &coeff in &expr.coeffs {
        gcd = std_gcd(gcd, coeff.wrapping_abs());
    }
    gcd
}

// `std::gcd`: the greatest common divisor of the absolute values, 0 for (0, 0).
fn std_gcd(m: i64, n: i64) -> i64 {
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

#[cfg(test)]
mod tests {
    use std::collections::HashSet;

    use cpsat_proto::{
        BoolArgumentProto, CpModelProto, LinearArgumentProto, LinearConstraintProto,
        NoOverlapConstraintProto,
    };
    use cpsat_testutil::parse_test_proto;

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

    // PENDING(phase 1): TEST(ConstraintCaseNameTest, TestFewCases), needs the fourth group of sat/cp_model_utils.h
    // PENDING(phase 1): TEST(ComputeInnerObjectiveTest, SimpleExample), needs the second group of sat/cp_model_utils.h
    // PENDING(phase 1): TEST(GetRefFromExpressionTest, BasicAPI), needs the second group of sat/cp_model_utils.h
    // PENDING(phase 1): TEST(AddLinearExpressionToLinearConstraintTest, BasicApi), needs the second group of sat/cp_model_utils.h
    // PENDING(phase 1): TEST(AddWeightedLiteralToLinearConstraintTest, BasicApi), needs the second group of sat/cp_model_utils.h
    // PENDING(phase 1): TEST(SafeAddLinearExpressionToLinearConstraintTest, BasicApi), needs the second group of sat/cp_model_utils.h
    // PENDING(phase 1): TEST(FingerprintTest, BasicApi), needs the fourth group of sat/cp_model_utils.h
    // PENDING(phase 1): TEST(ConvertCpModelProtoToCnfTest, BasicExample), needs the fourth group of sat/cp_model_utils.h
    // PENDING(phase 1): TEST(ConvertCpModelProtoToCnfTest, FixedVariables), needs the fourth group of sat/cp_model_utils.h
    // PENDING(phase 1): TEST(ConvertCpModelProtoToCnfTest, BoolAnd), needs the fourth group of sat/cp_model_utils.h
    // PENDING(phase 1): TEST(LargestConstraintTypeTest, IsCorrect), needs the fourth group of sat/cp_model_utils.h

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

    // PENDING(phase 1): TEST(ComputeFloatObjectiveValueTest, DirectedRoundingForMinAndMax), needs the second group of sat/cp_model_utils.h
    // PENDING(phase 1): TEST(ComputeFloatObjectiveValueTest, DirectedRoundingForZeroPointOnePlusTwo), needs the second group of sat/cp_model_utils.h
    // PENDING(phase 1): TEST(ComputeFloatObjectiveValueTest, ExactWithLargeInt64Solution), needs the second group of sat/cp_model_utils.h
    // PENDING(phase 1): TEST(CanonicalizeLinMaxTest, SortsTermsAndExpressions), needs the third group of sat/cp_model_utils.h
    // PENDING(phase 1): TEST(CanonicalizeLinMaxTest, CancelsZeroCoefficientTerms), needs the third group of sat/cp_model_utils.h
    // PENDING(phase 1): TEST(CanonicalizeLinMaxTest, CanonicalizesEnforcementLiterals), needs the third group of sat/cp_model_utils.h

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

    // PENDING(phase 1): TEST(CpModelUtilsTest, CanonicalizeLinearConstraintProto), needs the third group of sat/cp_model_utils.h

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
}
