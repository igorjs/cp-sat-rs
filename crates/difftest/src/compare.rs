//! The parts of the differential run that need no C++: what counts as a
//! disagreement, the summary and the rule that turns it into an exit code.

use std::{
    cell::Cell,
    collections::BTreeMap,
    fmt::Debug,
    panic::{self, AssertUnwindSafe, catch_unwind},
};

use cpsat_proto::CpSolverStatus;

/// How many of the first variables of a model get a changed value.
pub const CHECKED_VARIABLES: usize = 20;

const MAX_DETAIL_CHARS: usize = 200;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Comparison {
    Parse,
    Print,
    Validate,
    Check,
}

impl Comparison {
    fn name(self) -> &'static str {
        match self {
            Comparison::Parse => "parse",
            Comparison::Print => "print",
            Comparison::Validate => "validate",
            Comparison::Check => "check",
        }
    }
}

/// One place where the Rust code and the reference solver gave different answers.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Disagreement {
    pub comparison: Comparison,
    pub detail: String,
}

thread_local! {
    static IN_GUARD: Cell<bool> = const { Cell::new(false) };
}

/// Keeps the message of a panic that `guarded` catches out of the output. A
/// panic anywhere else still reaches the hook that was installed before.
pub fn silence_guarded_panics() {
    let previous = panic::take_hook();
    panic::set_hook(Box::new(move |info| {
        if !IN_GUARD.get() {
            previous(info);
        }
    }));
}

/// Runs `call` and returns its value, or the message of the panic it raised.
pub fn guarded<T>(call: impl FnOnce() -> T) -> Result<T, String> {
    let outer = IN_GUARD.replace(true);
    let caught = catch_unwind(AssertUnwindSafe(call));
    IN_GUARD.set(outer);
    caught.map_err(|payload| {
        payload
            .downcast_ref::<String>()
            .cloned()
            .or_else(|| {
                payload
                    .downcast_ref::<&str>()
                    .map(|text| (*text).to_owned())
            })
            .unwrap_or_else(|| "a panic with no text".to_owned())
    })
}

fn shorten(text: String) -> String {
    match text.char_indices().nth(MAX_DETAIL_CHARS) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text,
    }
}

/// Compares what the reference gives with what the Rust call gave, which is an
/// `Err` holding the panic message when the Rust call panicked.
pub fn agree<T: PartialEq + Debug>(
    comparison: Comparison,
    what: &str,
    reference: &T,
    rust: Result<T, String>,
) -> Option<Disagreement> {
    let detail = match rust {
        Ok(value) if value == *reference => return None,
        Ok(value) => shorten(format!(
            "{what}: the reference gives {reference:?}, Rust gives {value:?}"
        )),
        Err(message) => shorten(format!(
            "{what}: the reference gives {reference:?}, Rust panicked: {message}"
        )),
    };
    Some(Disagreement { comparison, detail })
}

/// The line that names a disagreement and the block it was found in.
pub fn report_disagreement(file: &str, line: usize, disagreement: &Disagreement) -> String {
    format!(
        "{file}:{line}: {} disagreement: {}",
        disagreement.comparison.name(),
        disagreement.detail
    )
}

/// Returns a value of the domain other than `current`, or `None` when the
/// domain holds no other value. `domain` is a flat list of closed intervals.
pub fn changed_value(domain: &[i64], current: i64) -> Option<i64> {
    let next = domain
        .as_chunks::<2>()
        .0
        .iter()
        .find(|[_, high]| current < *high)
        .map(|[low, _]| (*low).max(current + 1));
    next.or_else(|| domain.first().copied().filter(|&lowest| lowest != current))
}

/// What the check of one solution found.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct CheckResult {
    pub disagreements: Vec<Disagreement>,
    pub changed: usize,
    pub rejected: usize,
}

/// Checks that `rust` accepts `solution`, then that `rust` and `reference` give
/// the same verdict on `solution` with one variable changed, for each of the
/// first `CHECKED_VARIABLES` variables. `domains` holds one domain for each variable.
pub fn check_solution(
    solution: &[i64],
    domains: &[&[i64]],
    rust: impl Fn(&[i64]) -> Result<bool, String>,
    reference: impl Fn(&[i64]) -> Result<bool, String>,
) -> CheckResult {
    let mut result = CheckResult::default();
    result.disagreements.extend(agree(
        Comparison::Check,
        "the solution of the reference",
        &true,
        rust(solution),
    ));
    let mut changed = solution.to_vec();
    for (index, domain) in domains.iter().enumerate().take(CHECKED_VARIABLES) {
        let Some(value) = changed_value(domain, solution[index]) else {
            continue;
        };
        changed[index] = value;
        result.changed += 1;
        match reference(&changed) {
            Ok(expected) => {
                result.rejected += usize::from(!expected);
                result.disagreements.extend(agree(
                    Comparison::Check,
                    &format!("variable {index} set to {value}"),
                    &expected,
                    rust(&changed),
                ));
            }
            Err(message) => result.disagreements.push(Disagreement {
                comparison: Comparison::Check,
                detail: shorten(format!(
                    "variable {index} set to {value}: the reference failed: {message}"
                )),
            }),
        }
        changed[index] = solution[index];
    }
    result
}

/// How a model ended. `Skipped` and `Invalid` are not solver statuses: the first
/// is a block that is no `CpModelProto`, the second a model that at least one of
/// the two validations of the reference rejects. A model that could not be
/// compared because the bridge failed also ends `Invalid`, with a disagreement.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum ModelStatus {
    Skipped,
    Invalid,
    Unknown,
    ModelInvalid,
    Infeasible,
    Feasible,
    Optimal,
}

impl ModelStatus {
    pub fn from_solver(status: CpSolverStatus) -> Self {
        match status {
            CpSolverStatus::Unknown => ModelStatus::Unknown,
            CpSolverStatus::ModelInvalid => ModelStatus::ModelInvalid,
            CpSolverStatus::Infeasible => ModelStatus::Infeasible,
            CpSolverStatus::Feasible => ModelStatus::Feasible,
            CpSolverStatus::Optimal => ModelStatus::Optimal,
        }
    }

    pub fn name(self) -> &'static str {
        match self {
            ModelStatus::Skipped => "SKIPPED",
            ModelStatus::Invalid => "INVALID",
            ModelStatus::Unknown => CpSolverStatus::Unknown.as_str_name(),
            ModelStatus::ModelInvalid => CpSolverStatus::ModelInvalid.as_str_name(),
            ModelStatus::Infeasible => CpSolverStatus::Infeasible.as_str_name(),
            ModelStatus::Feasible => CpSolverStatus::Feasible.as_str_name(),
            ModelStatus::Optimal => CpSolverStatus::Optimal.as_str_name(),
        }
    }
}

/// The numbers a run must stay inside to pass.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Limits {
    pub max_unknown: usize,
    pub min_compared: usize,
    pub min_changed: usize,
    pub min_rejected: usize,
}

/// The tally of a run.
#[derive(Debug, Default)]
pub struct Summary {
    counts: BTreeMap<ModelStatus, usize>,
    disagreements: usize,
    changed: usize,
    rejected: usize,
}

impl Summary {
    pub fn record(&mut self, status: ModelStatus, disagreements: usize) {
        *self.counts.entry(status).or_default() += 1;
        self.disagreements += disagreements;
    }

    pub fn record_check(&mut self, result: &CheckResult) {
        self.changed += result.changed;
        self.rejected += result.rejected;
    }

    pub fn count(&self, status: ModelStatus) -> usize {
        self.counts.get(&status).copied().unwrap_or(0)
    }

    /// The models that were not skipped.
    pub fn compared(&self) -> usize {
        self.counts.values().sum::<usize>() - self.count(ModelStatus::Skipped)
    }

    pub fn disagreements(&self) -> usize {
        self.disagreements
    }

    /// One line for each status, then the totals.
    pub fn render(&self) -> String {
        let mut text = String::new();
        for (status, count) in &self.counts {
            text.push_str(&format!("{}: {count}\n", status.name()));
        }
        text.push_str(&format!(
            "compared: {}\ndisagreements: {}\nchanged solutions: {} ({} rejected by the reference)\n",
            self.compared(),
            self.disagreements,
            self.changed,
            self.rejected
        ));
        text
    }

    /// The reasons the run fails, empty when it passes.
    pub fn failures(&self, limits: &Limits) -> Vec<String> {
        let mut reasons = Vec::new();
        if self.disagreements > 0 {
            reasons.push(format!("{} disagreement(s)", self.disagreements));
        }
        let unknown = self.count(ModelStatus::Unknown);
        if unknown > limits.max_unknown {
            reasons.push(format!(
                "{unknown} models ended UNKNOWN, the limit is {}",
                limits.max_unknown
            ));
        }
        let compared = self.compared();
        if compared == 0 || compared < limits.min_compared {
            reasons.push(format!(
                "{compared} models were compared, at least {} are needed",
                limits.min_compared.max(1)
            ));
        }
        if self.changed < limits.min_changed {
            reasons.push(format!(
                "{} changed solutions were checked, at least {} are needed",
                self.changed, limits.min_changed
            ));
        }
        if self.rejected < limits.min_rejected {
            reasons.push(format!(
                "{} changed solutions were rejected by the reference, at least {} are needed",
                self.rejected, limits.min_rejected
            ));
        }
        reasons
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const LIMITS: Limits = Limits {
        max_unknown: 2,
        min_compared: 3,
        min_changed: 0,
        min_rejected: 0,
    };

    const SOLUTION_LIMITS: Limits = Limits {
        min_changed: 10,
        min_rejected: 4,
        ..LIMITS
    };

    fn summary_with_checks(changed: usize, rejected: usize) -> Summary {
        let mut summary = summary_of(&[ModelStatus::Optimal; 3]);
        summary.record_check(&CheckResult {
            disagreements: Vec::new(),
            changed,
            rejected,
        });
        summary
    }

    fn summary_of(statuses: &[ModelStatus]) -> Summary {
        let mut summary = Summary::default();
        for status in statuses {
            summary.record(*status, 0);
        }
        summary
    }

    #[test]
    fn a_solution_with_one_value_changed_is_a_disagreement_when_the_checker_accepts_it() {
        // Arrange
        let domains: Vec<&[i64]> = vec![&[0, 5], &[0, 5]];
        let accept_everything = |_: &[i64]| Ok(true);
        let reject_a_changed_value = |values: &[i64]| Ok(values == [1, 1]);

        // Act
        let result = check_solution(&[1, 1], &domains, accept_everything, reject_a_changed_value);

        // Assert
        assert_eq!(result.changed, 2);
        assert_eq!(result.rejected, 2);
        assert_eq!(result.disagreements.len(), 2);
        assert_eq!(result.disagreements[0].comparison, Comparison::Check);
        assert!(
            result.disagreements[0]
                .detail
                .contains("variable 0 set to 2")
        );
    }

    #[test]
    fn matching_verdicts_are_no_disagreement() {
        // Arrange
        let domains: Vec<&[i64]> = vec![&[0, 5]];
        let verdict = |values: &[i64]| Ok(values[0] == 1);

        // Act
        let result = check_solution(&[1], &domains, verdict, verdict);

        // Assert
        assert!(result.disagreements.is_empty());
        assert_eq!(result.rejected, 1);
    }

    #[test]
    fn a_checker_that_rejects_the_solution_of_the_reference_is_a_disagreement() {
        // Arrange
        let domains: Vec<&[i64]> = vec![&[0, 0]];

        // Act
        let result = check_solution(&[0], &domains, |_| Ok(false), |_| Ok(false));

        // Assert
        assert_eq!(result.disagreements.len(), 1);
        assert!(result.disagreements[0].detail.contains("the solution"));
    }

    #[test]
    fn a_panic_in_the_checker_is_a_disagreement_with_its_message() {
        // Arrange
        let domains: Vec<&[i64]> = vec![&[0, 5]];
        let panics = |_: &[i64]| guarded(|| -> bool { panic!("overflow in a sum") });

        // Act
        let result = check_solution(&[1], &domains, panics, |_| Ok(true));

        // Assert
        assert!(result.disagreements[0].detail.contains("overflow in a sum"));
    }

    #[test]
    fn a_reference_that_fails_on_a_changed_solution_is_a_disagreement() {
        // Arrange
        let domains: Vec<&[i64]> = vec![&[0, 5]];
        let reference = |values: &[i64]| {
            if values == [1] {
                Ok(true)
            } else {
                Err("the reference failed".to_owned())
            }
        };

        // Act
        let result = check_solution(&[1], &domains, |_| Ok(true), reference);

        // Assert
        assert_eq!(result.disagreements.len(), 1);
        assert!(
            result.disagreements[0]
                .detail
                .contains("the reference failed")
        );
    }

    #[test]
    fn only_the_first_variables_are_changed() {
        // Arrange
        let domain: &[i64] = &[0, 5];
        let domains = vec![domain; CHECKED_VARIABLES + 5];
        let solution = vec![1; CHECKED_VARIABLES + 5];

        // Act
        let result = check_solution(&solution, &domains, |_| Ok(true), |_| Ok(true));

        // Assert
        assert_eq!(result.changed, CHECKED_VARIABLES);
    }

    #[test]
    fn changed_value_picks_another_value_of_the_domain() {
        // Arrange
        let cases: [(&[i64], i64, Option<i64>); 6] = [
            (&[0, 5], 2, Some(3)),
            (&[0, 5], 5, Some(0)),
            (&[0, 1, 10, 12], 1, Some(10)),
            (&[0, 1, 10, 12], 12, Some(0)),
            (&[7, 7], 7, None),
            (&[], 0, None),
        ];

        for (domain, current, expected) in cases {
            // Act
            let value = changed_value(domain, current);

            // Assert
            assert_eq!(value, expected, "domain {domain:?}, value {current}");
        }
    }

    #[test]
    fn changed_value_handles_the_largest_value() {
        // Act, Assert
        assert_eq!(
            changed_value(&[i64::MIN, i64::MAX], i64::MAX),
            Some(i64::MIN)
        );
    }

    #[test]
    fn agree_reports_a_difference_and_a_panic() {
        // Act
        let same = agree(Comparison::Print, "text", &"a", Ok("a"));
        let differ = agree(Comparison::Print, "text", &"a", Ok("b"));
        let panicked = agree(Comparison::Parse, "text", &"a", Err("boom".to_owned()));

        // Assert
        assert_eq!(same, None);
        assert!(differ.unwrap().detail.contains("Rust gives \"b\""));
        assert!(panicked.unwrap().detail.contains("Rust panicked: boom"));
    }

    #[test]
    fn a_long_detail_is_shortened() {
        // Arrange
        let long = "x".repeat(MAX_DETAIL_CHARS * 2);

        // Act
        let detail = agree(Comparison::Print, "text", &long, Ok(String::new()))
            .unwrap()
            .detail;

        // Assert
        assert!(detail.ends_with("..."));
        assert!(detail.len() < MAX_DETAIL_CHARS + 4);
    }

    #[test]
    fn guarded_returns_the_message_of_each_kind_of_panic() {
        // Act
        let fixed = guarded(|| -> () { panic!("fixed text") });
        let formatted = guarded(|| -> () { panic!("value {}", 7) });
        let other = guarded(|| -> () { std::panic::panic_any(7_i32) });
        let fine = guarded(|| 3);

        // Assert
        assert_eq!(fixed, Err("fixed text".to_owned()));
        assert_eq!(formatted, Err("value 7".to_owned()));
        assert_eq!(other, Err("a panic with no text".to_owned()));
        assert_eq!(fine, Ok(3));
    }

    #[test]
    fn guarded_leaves_the_flag_of_the_caller_as_it_found_it() {
        // Arrange
        let outside = IN_GUARD.get();

        // Act
        let _ = guarded(|| IN_GUARD.get());
        let _ = guarded(|| panic!("caught"));
        let nested = guarded(|| {
            let _ = guarded(|| 1);
            IN_GUARD.get()
        });

        // Assert
        assert!(!outside);
        assert!(!IN_GUARD.get());
        assert_eq!(nested, Ok(true));
    }

    #[test]
    fn the_report_names_the_file_the_line_and_the_comparison() {
        // Arrange
        let disagreement = Disagreement {
            comparison: Comparison::Validate,
            detail: "detail".to_owned(),
        };

        // Act
        let line = report_disagreement("a_test.cc", 42, &disagreement);

        // Assert
        assert_eq!(line, "a_test.cc:42: validate disagreement: detail");
    }

    #[test]
    fn a_summary_within_the_limits_passes() {
        // Arrange
        let summary = summary_of(&[
            ModelStatus::Optimal,
            ModelStatus::Unknown,
            ModelStatus::Unknown,
            ModelStatus::Skipped,
        ]);

        // Act
        let failures = summary.failures(&LIMITS);

        // Assert
        assert_eq!(failures, Vec::<String>::new());
    }

    #[test]
    fn a_summary_with_one_unknown_above_the_limit_fails() {
        // Arrange
        let summary = summary_of(&[
            ModelStatus::Unknown,
            ModelStatus::Unknown,
            ModelStatus::Unknown,
        ]);

        // Act
        let failures = summary.failures(&LIMITS);

        // Assert
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("3 models ended UNKNOWN"));
    }

    #[test]
    fn a_summary_with_a_disagreement_fails() {
        // Arrange
        let mut summary = summary_of(&[ModelStatus::Optimal, ModelStatus::Optimal]);
        summary.record(ModelStatus::Invalid, 2);

        // Act
        let failures = summary.failures(&LIMITS);

        // Assert
        assert_eq!(failures, vec!["2 disagreement(s)".to_owned()]);
    }

    #[test]
    fn changed_solutions_at_the_floor_pass_and_one_below_fails() {
        // Arrange
        let at_floor = summary_with_checks(10, 4);
        let below = summary_with_checks(9, 4);

        // Act
        let passing = at_floor.failures(&SOLUTION_LIMITS);
        let failing = below.failures(&SOLUTION_LIMITS);

        // Assert
        assert_eq!(passing, Vec::<String>::new());
        assert_eq!(failing.len(), 1);
        assert!(failing[0].contains("9 changed solutions"));
    }

    #[test]
    fn rejected_solutions_at_the_floor_pass_and_one_below_fails() {
        // Arrange
        let below = summary_with_checks(10, 3);

        // Act
        let failing = below.failures(&SOLUTION_LIMITS);

        // Assert
        assert_eq!(failing.len(), 1);
        assert!(failing[0].contains("3 changed solutions were rejected"));
    }

    #[test]
    fn a_run_with_no_check_at_all_fails_both_floors() {
        // Arrange
        let summary = summary_with_checks(0, 0);

        // Act
        let failing = summary.failures(&SOLUTION_LIMITS);

        // Assert
        assert_eq!(failing.len(), 2);
    }

    #[test]
    fn an_empty_corpus_fails_even_with_no_least_count() {
        // Arrange
        let limits = Limits {
            max_unknown: 0,
            min_compared: 0,
            ..LIMITS
        };

        // Act
        let failures = Summary::default().failures(&limits);

        // Assert
        assert_eq!(failures.len(), 1);
        assert!(failures[0].contains("0 models were compared"));
    }

    #[test]
    fn skipped_blocks_do_not_count_as_compared() {
        // Arrange
        let summary = summary_of(&[
            ModelStatus::Skipped,
            ModelStatus::Skipped,
            ModelStatus::Skipped,
            ModelStatus::Optimal,
        ]);

        // Act
        let failures = summary.failures(&LIMITS);

        // Assert
        assert_eq!(summary.compared(), 1);
        assert!(failures[0].contains("at least 3 are needed"));
    }

    #[test]
    fn the_rendering_lists_each_status_and_the_totals() {
        // Arrange
        let mut summary = summary_of(&[ModelStatus::Optimal, ModelStatus::Skipped]);
        summary.record_check(&CheckResult {
            disagreements: Vec::new(),
            changed: 4,
            rejected: 3,
        });

        // Act
        let text = summary.render();

        // Assert
        assert_eq!(
            text,
            "SKIPPED: 1\nOPTIMAL: 1\ncompared: 1\ndisagreements: 0\n\
             changed solutions: 4 (3 rejected by the reference)\n"
        );
    }

    #[test]
    fn each_solver_status_has_a_name() {
        // Arrange
        let pairs = [
            (CpSolverStatus::Unknown, "UNKNOWN"),
            (CpSolverStatus::ModelInvalid, "MODEL_INVALID"),
            (CpSolverStatus::Infeasible, "INFEASIBLE"),
            (CpSolverStatus::Feasible, "FEASIBLE"),
            (CpSolverStatus::Optimal, "OPTIMAL"),
        ];

        for (status, name) in pairs {
            // Act, Assert
            assert_eq!(ModelStatus::from_solver(status).name(), name);
        }
        assert_eq!(ModelStatus::Invalid.name(), "INVALID");
    }
}
