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

/// The characters the text form adds around the type and size note: `# ` and a newline.
/// Upstream keeps the same `+ 3` and pins it with unit tests.
const TEXT_FORM_EXTRA_CHARS: usize = 3;
const BYTES_PER_BASE64_GROUP: usize = 3;
const BITS_PER_BYTE: usize = 8;
const BITS_PER_BASE64_DIGIT: usize = 6;
const BASE64_DIGIT_MASK: u32 = 0x3f;
/// The shift that brings the first digit of a 24 bit group to the lowest bits.
const FIRST_BASE64_DIGIT_SHIFT: usize =
    BITS_PER_BYTE * BYTES_PER_BASE64_GROUP - BITS_PER_BASE64_DIGIT;

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
///
/// Differs from C++ for a double field set to -0.0: prost skips it when it encodes, so
/// the size and the text miss a field the C++ text prints as `-0`.
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
            if text.len() + output.len() + TEXT_FORM_EXTRA_CHARS <= max_chars
                && text.matches('\n').count() < max_lines
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
        Ok(()) => {
            // Upstream writes the error text on every call, so a success clears it.
            error_out.clear();
            true
        }
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
    for chunk in bytes.chunks(BYTES_PER_BASE64_GROUP) {
        let group = chunk
            .iter()
            .fold(0u32, |acc, byte| acc << BITS_PER_BYTE | u32::from(*byte))
            << (BITS_PER_BYTE * (BYTES_PER_BASE64_GROUP - chunk.len()));
        for index in 0..=chunk.len() {
            out.push(char::from(
                ALPHABET[(group >> (FIRST_BASE64_DIGIT_SHIFT - BITS_PER_BASE64_DIGIT * index)
                    & BASE64_DIGIT_MASK) as usize],
            ));
        }
        out.extend(std::iter::repeat_n(
            '=',
            BYTES_PER_BASE64_GROUP - chunk.len(),
        ));
    }
    out
}

#[cfg(test)]
mod tests;
