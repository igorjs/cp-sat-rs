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
// Ported from ortools/port/proto_utils.h at commit c931f5f7. Changed: translated from C++ to Rust.

//! Debug strings, enum names and text format helpers for protobuf messages.

use std::sync::OnceLock;

use cpsat_proto::FILE_DESCRIPTOR_SET;
use prost::{Message, Name};
use prost_reflect::{DescriptorPool, DynamicMessage, MessageDescriptor};

use crate::text_format_printer;

/// Stands for `kint32max`, the default of both limits of `protobuf_debug_string`.
pub const NO_LIMIT: i32 = i32::MAX;

fn pool() -> &'static DescriptorPool {
    static POOL: OnceLock<DescriptorPool> = OnceLock::new();
    POOL.get_or_init(|| {
        DescriptorPool::decode(FILE_DESCRIPTOR_SET)
            .expect("the descriptor set of cpsat-proto decodes")
    })
}

fn descriptor_of<P: Name>() -> MessageDescriptor {
    let name = P::full_name();
    pool()
        .get_message_by_name(&name)
        .unwrap_or_else(|| panic!("no message {name} in the descriptor set"))
}

fn dynamic_of<P: Message + Name>(message: &P) -> DynamicMessage {
    DynamicMessage::decode(descriptor_of::<P>(), message.encode_to_vec().as_slice())
        .expect("a message encoded by prost decodes")
}

fn print_text<P: Message + Name>(message: &P, single_line: bool) -> String {
    text_format_printer::print(&dynamic_of(message), single_line)
}

/// Returns a debug string of the given message, suitable for logging.
pub fn protobuf_debug_string<P: Message + Name + Default>(
    message: &P,
    max_chars: i32,
    max_lines: i32,
) -> String {
    let serialized = message.encode_to_vec();
    let mut output = format!("[{} of {} bytes]", P::full_name(), serialized.len());
    if let Ok(max_chars) = usize::try_from(max_chars) {
        if let Ok(max_lines) = usize::try_from(max_lines)
            && serialized.len() < max_chars
        {
            let text = print_text(message, false);
            if text.len() + output.len() + 3 <= max_chars && text.matches('\n').count() < max_lines
            {
                return format!("# {output}\n{text}");
            }
        }
        if serialized.len() < max_chars {
            let with_base64 = format!("{output} {}", base64(&serialized));
            if with_base64.len() <= max_chars {
                output = with_base64;
            }
        }
    }
    output
}

pub fn protobuf_short_debug_string<P: Message + Name + Default>(message: &P) -> String {
    print_text(message, true).trim_end().to_owned()
}

pub fn proto_enum_to_string(enum_name: &str, enum_value: impl Into<i32>) -> String {
    let number = enum_value.into();
    let descriptor = pool()
        .get_enum_by_name(enum_name)
        .unwrap_or_else(|| panic!("no enum {enum_name} in the descriptor set"));
    match descriptor.get_value(number) {
        Some(value) => value.name().to_owned(),
        None => format!(
            "Invalid enum value of: {number} for enum type: {}",
            descriptor.name()
        ),
    }
}

pub fn protobuf_text_format_merge_from_string<P: Message + Name + Default>(
    proto_text_string: &str,
    proto: &mut P,
) -> bool {
    merge_text(proto_text_string, proto).is_ok()
}

pub fn protobuf_parse_text_proto_for_flag<P: Message + Name + Default>(
    text: &str,
    message_out: &mut P,
    error_out: &mut String,
) -> bool {
    *message_out = P::default();
    match merge_text(text, message_out) {
        Ok(()) => true,
        Err(error) => {
            *error_out = format!("{error}\n");
            false
        }
    }
}

pub fn protobuf_text_format_print_to_string_for_flag<P: Message + Name + Default>(
    proto: &P,
) -> String {
    protobuf_short_debug_string(proto)
}

fn merge_text<P: Message + Name + Default>(text: &str, proto: &mut P) -> Result<(), String> {
    let mut dynamic = dynamic_of(proto);
    dynamic
        .merge_text_format(text)
        .map_err(|error| error.to_string())?;
    *proto = dynamic
        .transcode_to::<P>()
        .map_err(|error| error.to_string())?;
    Ok(())
}

fn base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let group = chunk
            .iter()
            .fold(0u32, |acc, byte| acc << 8 | u32::from(*byte))
            << (8 * (3 - chunk.len()));
        for index in 0..=chunk.len() {
            out.push(char::from(
                ALPHABET[(group >> (18 - 6 * index) & 0x3f) as usize],
            ));
        }
        out.extend(std::iter::repeat_n('=', 3 - chunk.len()));
    }
    out
}

#[cfg(test)]
mod tests {
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
        let cases = cases();

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
        let cases = cases();

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
    fn parse_text_proto_for_flag_reads_back_the_long_text_of_each_fixture() {
        // Arrange
        let cases = cases();

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
        let cases = cases();

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
}
