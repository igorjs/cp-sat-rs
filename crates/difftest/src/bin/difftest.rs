//! Compares the Rust parser, printer, model validation and solution check with
//! the reference solver on the models in the upstream solver and checker tests.
//!
//! The models are read at run time from `upstream/`, which
//! `scripts/fetch-upstream.sh` fills. The run exits with a failure code on any
//! disagreement and when it compared too few models or too many ended `UNKNOWN`.

use std::{
    fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use cpsat_difftest::{
    compare::{
        Comparison, Disagreement, Limits, ModelStatus, Summary, agree, check_solution, guarded,
        report_disagreement, silence_guarded_panics,
    },
    extract::extract,
    reference_debug_string, reference_parse_text, reference_solution_is_feasible, reference_solve,
    reference_validate, reference_validate_input,
};
use cpsat_port::proto_utils::protobuf_short_debug_string;
use cpsat_proto::{CpModelProto, CpSolverResponse, SatParameters};
use cpsat_sat::cp_model_checker::{
    solution_is_feasible, validate_cp_model, validate_input_cp_model,
};
use cpsat_testutil::parse_test_proto;
use prost::{Message, Name};

/// The test sources that hold the models, relative to the repository root.
const CORPUS_FILES: [&str; 2] = [
    "upstream/ortools/sat/cp_model_solver_test.cc",
    "upstream/ortools/sat/cp_model_checker_test.cc",
];

const SOLVE_WORKERS: i32 = 1;
const SOLVE_SEED: i32 = 1;
/// The deterministic time, in seconds, that the reference may spend on a model.
const MAX_DETERMINISTIC_TIME: f64 = 0.01;
/// One model above the 0 that ended `UNKNOWN` in the first full run.
const MAX_UNKNOWN: usize = 1;
/// The models of the first full run, which are not skipped.
const MIN_COMPARED: usize = 200;
/// The changed solutions of the first full run.
const MIN_CHANGED: usize = 443;
/// The changed solutions of the first full run that the reference rejects.
const MIN_REJECTED: usize = 246;
const LIMITS: Limits = Limits {
    max_unknown: MAX_UNKNOWN,
    min_compared: MIN_COMPARED,
    min_changed: MIN_CHANGED,
    min_rejected: MIN_REJECTED,
};

type Check = Result<(), Disagreement>;

fn reference_failed<T>(
    comparison: Comparison,
    what: &str,
    result: Result<T, cxx::Exception>,
) -> Result<T, Disagreement> {
    result.map_err(|error| Disagreement {
        comparison,
        detail: format!("{what}: the reference failed: {error}"),
    })
}

/// The first disagreement of the two comparisons, if any.
fn first(found: [Option<Disagreement>; 2]) -> Option<Disagreement> {
    found.into_iter().flatten().next()
}

fn compare_parse(text: &str, bytes: &[u8], model: &CpModelProto) -> Option<Disagreement> {
    let rust = match guarded(|| parse_test_proto::<CpModelProto>(text)) {
        Ok(rust) => rust,
        Err(message) => {
            return Some(Disagreement {
                comparison: Comparison::Parse,
                detail: format!("Rust panicked: {message}"),
            });
        }
    };
    // The bytes catch what a message comparison cannot: a double of -0.0 is equal to 0.0.
    first([
        agree(Comparison::Parse, "message", model, Ok(rust.clone())),
        agree(
            Comparison::Parse,
            "encoded bytes",
            &bytes.to_vec(),
            Ok(rust.encode_to_vec()),
        ),
    ])
}

fn compare_print(bytes: &[u8], model: &CpModelProto) -> Check {
    let reference = reference_failed(
        Comparison::Print,
        "short debug string",
        reference_debug_string(&CpModelProto::full_name(), bytes, true, 0, 0),
    )?;
    let rust = guarded(|| protobuf_short_debug_string(model));
    agree(Comparison::Print, "short debug string", &reference, rust).map_or(Ok(()), Err)
}

/// What the two validations found.
struct Validation {
    disagreements: Vec<Disagreement>,
    reference_accepts: bool,
}

fn compare_validate(bytes: &[u8], model: &CpModelProto) -> Result<Validation, Disagreement> {
    let parameters = SatParameters::default();
    let model_text = reference_failed(
        Comparison::Validate,
        "validate_cp_model",
        reference_validate(bytes),
    )?;
    let input_text = reference_failed(
        Comparison::Validate,
        "validate_input_cp_model",
        reference_validate_input(&parameters.encode_to_vec(), bytes),
    )?;
    let disagreements = [
        agree(
            Comparison::Validate,
            "validate_cp_model",
            &model_text,
            guarded(|| validate_cp_model(model, false)),
        ),
        agree(
            Comparison::Validate,
            "validate_input_cp_model",
            &input_text,
            guarded(|| validate_input_cp_model(&parameters, model)),
        ),
    ]
    .into_iter()
    .flatten()
    .collect();
    Ok(Validation {
        disagreements,
        reference_accepts: model_text.is_empty() && input_text.is_empty(),
    })
}

fn solve_parameters() -> SatParameters {
    SatParameters {
        num_workers: Some(SOLVE_WORKERS),
        random_seed: Some(SOLVE_SEED),
        max_deterministic_time: Some(MAX_DETERMINISTIC_TIME),
        ..Default::default()
    }
}

/// What one model gave: how it ended, and every disagreement found on the way.
struct Outcome {
    status: ModelStatus,
    disagreements: Vec<Disagreement>,
    check: Option<cpsat_difftest::compare::CheckResult>,
    /// Why the block was skipped, for the line of the run.
    skip_reason: Option<String>,
}

impl Outcome {
    fn of(status: ModelStatus) -> Self {
        Outcome {
            status,
            disagreements: Vec::new(),
            check: None,
            skip_reason: None,
        }
    }

    fn disagreeing(mut self, found: impl IntoIterator<Item = Disagreement>) -> Self {
        self.disagreements.extend(found);
        self
    }
}

fn solve_and_check(bytes: &[u8], model: &CpModelProto) -> Outcome {
    let solved = reference_failed(
        Comparison::Check,
        "solve",
        reference_solve(bytes, &solve_parameters().encode_to_vec()),
    )
    .and_then(|response| {
        CpSolverResponse::decode(response.as_slice()).map_err(|error| Disagreement {
            comparison: Comparison::Check,
            detail: format!("the answer of the reference does not decode: {error}"),
        })
    });
    let response = match solved {
        Ok(response) => response,
        Err(disagreement) => return Outcome::of(ModelStatus::Unknown).disagreeing([disagreement]),
    };
    let mut outcome = Outcome::of(ModelStatus::from_solver(response.status()));
    if outcome.status == ModelStatus::ModelInvalid {
        return outcome.disagreeing([Disagreement {
            comparison: Comparison::Check,
            detail: "the reference calls MODEL_INVALID a model that both validations accept"
                .to_owned(),
        }]);
    }
    let has_solution = matches!(outcome.status, ModelStatus::Feasible | ModelStatus::Optimal);
    if !has_solution && response.solution.is_empty() {
        return outcome;
    }
    let domains: Vec<&[i64]> = model
        .variables
        .iter()
        .map(|variable| variable.domain.as_slice())
        .collect();
    if response.solution.len() != domains.len() {
        return outcome.disagreeing([Disagreement {
            comparison: Comparison::Check,
            detail: format!(
                "the reference gives {} values for {} variables",
                response.solution.len(),
                domains.len()
            ),
        }]);
    }
    let result = check_solution(
        &response.solution,
        &domains,
        |values| guarded(|| solution_is_feasible(model, values, None, None)),
        |values| reference_solution_is_feasible(bytes, values).map_err(|error| error.to_string()),
    );
    outcome.disagreements.extend(result.disagreements.clone());
    outcome.check = Some(result);
    outcome
}

/// A block the reference does not parse as a `CpModelProto`: Rust must reject it too.
fn skipped(text: &str, reference_error: &str) -> Outcome {
    let mut outcome = Outcome::of(ModelStatus::Skipped);
    outcome.skip_reason = Some(reference_error.to_owned());
    if guarded(|| parse_test_proto::<CpModelProto>(text)).is_ok() {
        outcome.disagreements.push(Disagreement {
            comparison: Comparison::Parse,
            detail: format!(
                "Rust parses a text that the reference rejects: {}",
                reference_error
            ),
        });
    }
    outcome
}

fn compare_block(text: &str) -> Outcome {
    let bytes = match reference_parse_text(&CpModelProto::full_name(), text) {
        Ok(bytes) => bytes,
        Err(error) => return skipped(text, &error.to_string()),
    };
    let model = match CpModelProto::decode(bytes.as_slice()) {
        Ok(model) => model,
        Err(error) => {
            return Outcome::of(ModelStatus::Invalid).disagreeing([Disagreement {
                comparison: Comparison::Parse,
                detail: format!("the bytes of the reference do not decode: {error}"),
            }]);
        }
    };
    let mut disagreements: Vec<Disagreement> =
        compare_parse(text, &bytes, &model).into_iter().collect();
    disagreements.extend(compare_print(&bytes, &model).err());
    let validation = match compare_validate(&bytes, &model) {
        Ok(validation) => validation,
        Err(failure) => {
            return Outcome::of(ModelStatus::Invalid)
                .disagreeing(disagreements.into_iter().chain([failure]));
        }
    };
    disagreements.extend(validation.disagreements);
    if !validation.reference_accepts {
        return Outcome::of(ModelStatus::Invalid).disagreeing(disagreements);
    }
    solve_and_check(&bytes, &model).disagreeing(disagreements)
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn run() -> Result<ExitCode, String> {
    let mut summary = Summary::default();
    for relative in CORPUS_FILES {
        let path = repository_root().join(relative);
        let source = fs::read_to_string(&path).map_err(|error| {
            format!(
                "cannot read {}: {error}; scripts/fetch-upstream.sh fetches it",
                path.display()
            )
        })?;
        let file = path
            .file_name()
            .map_or_else(String::new, |name| name.to_string_lossy().into_owned());
        let blocks = extract(&source).map_err(|error| format!("{file}: {error}"))?;
        println!("{file}: {} blocks", blocks.len());
        for block in blocks {
            let outcome = compare_block(&block.text);
            match &outcome.skip_reason {
                Some(reason) => println!(
                    "{file}:{}: {} (reference: {})",
                    block.line,
                    outcome.status.name(),
                    reason.lines().next().unwrap_or_default()
                ),
                None => println!("{file}:{}: {}", block.line, outcome.status.name()),
            }
            for disagreement in &outcome.disagreements {
                println!("  {}", report_disagreement(&file, block.line, disagreement));
            }
            summary.record(outcome.status, outcome.disagreements.len());
            if let Some(check) = &outcome.check {
                summary.record_check(check);
            }
        }
    }
    print!("{}", summary.render());
    let failures = summary.failures(&LIMITS);
    for reason in &failures {
        eprintln!("failure: {reason}");
    }
    Ok(if failures.is_empty() {
        ExitCode::SUCCESS
    } else {
        ExitCode::FAILURE
    })
}

fn main() -> ExitCode {
    // A panic caught by `guarded` is reported as a disagreement, so its message
    // stays out of the output. Any other panic prints as usual.
    silence_guarded_panics();
    run().unwrap_or_else(|message| {
        eprintln!("error: {message}");
        ExitCode::FAILURE
    })
}
