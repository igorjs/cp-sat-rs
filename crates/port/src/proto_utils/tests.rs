use cpsat_proto::{
    ConstraintProto, CpModelProto, CpObjectiveProto, CpSolverStatus, IntegerVariableProto,
    LinearConstraintProto, SatParameters,
};
use cpsat_testutil::fixture::{self, Case};
use prost::Message;

use super::*;

const FIXTURE: &str = concat!(
    env!("CARGO_MANIFEST_DIR"),
    "/tests/fixtures/debug_strings.txt"
);
const PACKAGE: &str = "operations_research.sat.";
const STATUS_ENUM: &str = "operations_research.sat.CpSolverStatus";
const UNKNOWN_STATUS: i32 = 99;
const MIN_FIXTURE_MESSAGES: usize = 7;
const LIMITED_CASES: usize = 3;
/// A `CpObjectiveProto` whose `offset` is -0.0.
const NEGATIVE_ZERO_OFFSET: &str = "110000000000000080";
/// A `SatParameters` whose `initial_polarity` is 99, a number the enum does not define.
const UNKNOWN_ENUM_VALUE: &str = "1063";

/// Decodes the bytes of `$case` as the message type of its name, and evaluates
/// `$body` with it bound to `$message`.
macro_rules! with_message {
    ($case:expr, |$message:ident| $body:expr) => {{
        let bytes = fixture::from_hex($case.single("bytes"));
        match $case.single("message").strip_prefix(PACKAGE).unwrap() {
            "LinearConstraintProto" => {
                let $message = LinearConstraintProto::decode(bytes.as_slice()).unwrap();
                $body
            }
            "ConstraintProto" => {
                let $message = ConstraintProto::decode(bytes.as_slice()).unwrap();
                $body
            }
            "IntegerVariableProto" => {
                let $message = IntegerVariableProto::decode(bytes.as_slice()).unwrap();
                $body
            }
            "CpObjectiveProto" => {
                let $message = CpObjectiveProto::decode(bytes.as_slice()).unwrap();
                $body
            }
            "SatParameters" => {
                let $message = SatParameters::decode(bytes.as_slice()).unwrap();
                $body
            }
            "CpModelProto" => {
                let $message = CpModelProto::decode(bytes.as_slice()).unwrap();
                $body
            }
            other => panic!("the fixture holds a message {other} the test does not know"),
        }
    }};
}

fn empty_like<P: Default>(_: &P) -> P {
    P::default()
}

fn cases() -> Vec<Case> {
    fixture::read(FIXTURE)
}

fn matches_the_reference(case: &Case) -> bool {
    case.single("bytes") != NEGATIVE_ZERO_OFFSET
}

fn cases_with_reference_text() -> Vec<Case> {
    cases().into_iter().filter(matches_the_reference).collect()
}

/// The text of an unknown field has no field name, so no parser reads it back.
fn cases_that_parse_back() -> Vec<Case> {
    cases_with_reference_text()
        .into_iter()
        .filter(|case| case.single("bytes") != UNKNOWN_ENUM_VALUE)
        .collect()
}

fn limits_of(case: &Case) -> Vec<(i32, i32)> {
    case.all("limits")
        .iter()
        .map(|pair| {
            let (chars, lines) = pair.split_once(' ').unwrap();
            (chars.parse().unwrap(), lines.parse().unwrap())
        })
        .collect()
}

#[test]
fn short_text_equals_the_text_of_the_reference() {
    // Arrange
    let cases = cases_with_reference_text();

    for case in &cases {
        // Act
        let short = with_message!(case, |message| protobuf_short_debug_string(&message));

        // Assert
        assert_eq!(short, case.single("short"), "{}", case.single("message"));
    }
    assert!(cases.len() >= MIN_FIXTURE_MESSAGES);
}

#[test]
fn long_text_equals_the_text_of_the_reference() {
    // Arrange
    let cases = cases_with_reference_text();

    for case in &cases {
        // Act
        let long = with_message!(case, |message| protobuf_debug_string(
            &message, NO_LIMIT, NO_LIMIT
        ));

        // Assert
        assert_eq!(long, case.single("long"), "{}", case.single("message"));
    }
}

#[test]
fn limited_text_equals_the_text_of_the_reference_for_each_branch() {
    // Arrange
    let cases = cases();
    let limited: Vec<&Case> = cases
        .iter()
        .filter(|case| !case.all("limits").is_empty())
        .collect();

    for case in &limited {
        let limits = limits_of(case);
        let expected = case.all("limited");
        assert_eq!(limits.len(), expected.len());

        for ((max_chars, max_lines), expected) in limits.iter().zip(expected) {
            // Act
            let text = with_message!(case, |message| protobuf_debug_string(
                &message, *max_chars, *max_lines
            ));

            // Assert
            assert_eq!(text, expected, "limits {max_chars} {max_lines}");
        }
    }
    assert_eq!(limited.len(), LIMITED_CASES);
}

#[test]
fn proto_enum_to_string_returns_the_upstream_name() {
    // Arrange, Act
    let name = proto_enum_to_string(STATUS_ENUM, CpSolverStatus::Optimal);

    // Assert
    assert_eq!(name, "OPTIMAL");
}

#[test]
fn proto_enum_to_string_names_an_invalid_value() {
    // Arrange, Act
    let name = proto_enum_to_string(STATUS_ENUM, UNKNOWN_STATUS);

    // Assert
    assert_eq!(
        name,
        "Invalid enum value of: 99 for enum type: CpSolverStatus"
    );
}

#[test]
fn parse_text_proto_for_flag_fails_on_an_unknown_field_and_names_it() {
    // Arrange
    let mut message = CpModelProto::default();
    let mut error = String::new();

    // Act
    let parsed = protobuf_parse_text_proto_for_flag(
        "variables { no_such_field: 1 }",
        &mut message,
        &mut error,
    );

    // Assert
    assert!(!parsed);
    assert!(error.contains("no_such_field"), "{error}");
}

#[test]
fn parse_text_proto_for_flag_clears_the_error_of_an_earlier_call() {
    // Arrange
    let mut message = CpModelProto::default();
    let mut error = "an earlier failure".to_owned();

    // Act
    let parsed = protobuf_parse_text_proto_for_flag("name: \"m\"", &mut message, &mut error);

    // Assert
    assert!(parsed);
    assert_eq!(error, "");
}

/// A typed message cannot tell -0.0 from an absent field, since prost skips the
/// field when it encodes. The C++ text keeps `offset: -0` and counts 9 bytes.
#[test]
fn debug_string_of_a_negative_zero_double_differs_from_the_reference() {
    // Arrange
    let message = CpObjectiveProto {
        offset: -0.0,
        ..Default::default()
    };

    // Act
    let long = protobuf_debug_string(&message, NO_LIMIT, NO_LIMIT);

    // Assert
    assert_eq!(
        long,
        "# [operations_research.sat.CpObjectiveProto of 0 bytes]\n"
    );
}

#[test]
fn parse_text_proto_for_flag_reads_back_the_long_text_of_each_fixture() {
    // Arrange
    let cases = cases_that_parse_back();

    for case in &cases {
        // Act
        let (parsed, error, original) = with_message!(case, |original| {
            let text = protobuf_debug_string(&original, NO_LIMIT, NO_LIMIT);
            let mut parsed = empty_like(&original);
            let mut error = String::new();
            let ok = protobuf_parse_text_proto_for_flag(&text, &mut parsed, &mut error);
            (
                ok,
                error,
                parsed.encode_to_vec() == original.encode_to_vec(),
            )
        });

        // Assert
        assert!(parsed, "{error}");
        assert!(original, "{}", case.single("message"));
    }
}

#[test]
fn print_to_string_for_flag_parses_back_to_the_same_message() {
    // Arrange
    let cases = cases_that_parse_back();

    for case in &cases {
        // Act
        let same = with_message!(case, |original| {
            let text = protobuf_text_format_print_to_string_for_flag(&original);
            let mut parsed = empty_like(&original);
            let mut error = String::new();
            protobuf_parse_text_proto_for_flag(&text, &mut parsed, &mut error)
                && parsed.encode_to_vec() == original.encode_to_vec()
        });

        // Assert
        assert!(same, "{}", case.single("message"));
    }
}

#[test]
fn merge_from_string_adds_to_the_message_and_reports_bad_text() {
    // Arrange
    let mut model = CpModelProto {
        name: "kept".to_owned(),
        ..Default::default()
    };

    // Act
    let merged = protobuf_text_format_merge_from_string("variables { domain: 1 }", &mut model);
    let failed = protobuf_text_format_merge_from_string("variables {", &mut model);

    // Assert
    assert!(merged);
    assert!(!failed);
    assert_eq!(model.name, "kept");
    assert_eq!(model.variables.len(), 1);
}
