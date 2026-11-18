//! Prints a message in the protobuf text format exactly as the C++ `TextFormat::Printer`
//! does. `prost-reflect` prints repeated fields as lists and ends the text without a
//! newline, so its text differs from the C++ text.

use prost_reflect::{DynamicMessage, FieldDescriptor, Kind, Syntax, Value};

const INDENT: &str = "  ";
const DOUBLE_PRECISIONS: [usize; 2] = [15, 17];
const FLOAT_PRECISIONS: [usize; 2] = [6, 9];
/// A `%g` conversion switches to the scientific form below this decimal exponent.
const MIN_FIXED_EXPONENT: i32 = -4;
const FIRST_PRINTABLE: u8 = 0x20;
const DELETE: u8 = 0x7f;

/// Prints `message` over several lines, or on one line with a space after each field.
pub fn print(message: &DynamicMessage, single_line: bool) -> String {
    let mut printer = Printer {
        out: String::new(),
        single_line,
        depth: 0,
    };
    printer.message(message);
    printer.out
}

struct Printer {
    out: String,
    single_line: bool,
    depth: usize,
}

impl Printer {
    fn message(&mut self, message: &DynamicMessage) {
        let mut fields: Vec<(FieldDescriptor, &Value)> = message.fields().collect();
        fields.sort_by_key(|(field, _)| field.number());
        let mut unknown = Vec::new();
        for (field, value) in fields {
            let items = match value {
                Value::List(items) => items.as_slice(),
                single => std::slice::from_ref(single),
            };
            for item in items {
                match unknown_enum_number(&field, item) {
                    Some(number) => unknown.push((field.number(), number)),
                    None => self.field(&field, item),
                }
            }
        }
        for (field_number, value) in unknown {
            self.unknown_varint(field_number, value);
        }
    }

    fn unknown_varint(&mut self, field_number: u32, value: i32) {
        if !self.single_line {
            self.out.push_str(&INDENT.repeat(self.depth));
        }
        self.out.push_str(&format!("{field_number}: {value}"));
        self.out.push(if self.single_line { ' ' } else { '\n' });
    }

    fn field(&mut self, field: &FieldDescriptor, value: &Value) {
        if !self.single_line {
            self.out.push_str(&INDENT.repeat(self.depth));
        }
        self.out.push_str(field.name());
        if let Value::Message(inner) = value {
            if self.single_line {
                self.out.push_str(" { ");
                self.message(inner);
                self.out.push_str("} ");
            } else {
                self.out.push_str(" {\n");
                self.depth += 1;
                self.message(inner);
                self.depth -= 1;
                self.out.push_str(&INDENT.repeat(self.depth));
                self.out.push_str("}\n");
            }
        } else {
            self.out.push_str(": ");
            self.out.push_str(&scalar(field, value));
            self.out.push(if self.single_line { ' ' } else { '\n' });
        }
    }
}

/// The number of a value that the C++ parser of a proto2 file keeps as an unknown field,
/// because the closed enum of the field has no value with that number.
fn unknown_enum_number(field: &FieldDescriptor, value: &Value) -> Option<i32> {
    match (field.kind(), value) {
        (Kind::Enum(descriptor), Value::EnumNumber(number))
            if descriptor.parent_file().syntax() == Syntax::Proto2
                && descriptor.get_value(*number).is_none() =>
        {
            Some(*number)
        }
        _ => None,
    }
}

fn scalar(field: &FieldDescriptor, value: &Value) -> String {
    match value {
        Value::Bool(flag) => flag.to_string(),
        Value::I32(number) => number.to_string(),
        Value::I64(number) => number.to_string(),
        Value::U32(number) => number.to_string(),
        Value::U64(number) => number.to_string(),
        Value::F32(number) => float(f64::from(*number), FLOAT_PRECISIONS, |text| {
            text.parse::<f32>().is_ok_and(|parsed| parsed == *number)
        }),
        Value::F64(number) => float(*number, DOUBLE_PRECISIONS, |text| {
            text.parse::<f64>().is_ok_and(|parsed| parsed == *number)
        }),
        Value::String(text) => quoted(text.as_bytes()),
        Value::Bytes(bytes) => quoted(bytes),
        Value::EnumNumber(number) => match field.kind() {
            Kind::Enum(descriptor) => descriptor
                .get_value(*number)
                .map_or_else(|| number.to_string(), |named| named.name().to_owned()),
            _ => number.to_string(),
        },
        Value::Message(_) | Value::List(_) | Value::Map(_) => {
            unreachable!("the printer handles messages and lists before it reads a scalar")
        }
    }
}

/// `CEscape` between double quotes: a byte outside the printable range is an octal escape.
fn quoted(bytes: &[u8]) -> String {
    let mut out = String::from("\"");
    for &byte in bytes {
        match byte {
            b'\n' => out.push_str("\\n"),
            b'\r' => out.push_str("\\r"),
            b'\t' => out.push_str("\\t"),
            b'"' => out.push_str("\\\""),
            b'\'' => out.push_str("\\'"),
            b'\\' => out.push_str("\\\\"),
            FIRST_PRINTABLE..DELETE => out.push(char::from(byte)),
            _ => out.push_str(&format!("\\{byte:03o}")),
        }
    }
    out.push('"');
    out
}

/// The first `%g` form, in the order of `precisions`, that reads back as the same
/// number, which is how the C++ `SimpleDtoa` and `SimpleFtoa` choose.
fn float(value: f64, precisions: [usize; 2], reads_back: impl Fn(&str) -> bool) -> String {
    if value.is_nan() {
        return "nan".to_owned();
    }
    if value.is_infinite() {
        return if value < 0.0 { "-inf" } else { "inf" }.to_owned();
    }
    let [short, long] = precisions;
    let text = format_g(value, short);
    if reads_back(&text) {
        text
    } else {
        format_g(value, long)
    }
}

/// C `printf("%.{precision}g", value)` for a finite number.
fn format_g(value: f64, precision: usize) -> String {
    let scientific = format!("{value:.*e}", precision - 1);
    let (mantissa, exponent) = scientific
        .split_once('e')
        .expect("a number in scientific form has an exponent");
    let exponent: i32 = exponent.parse().expect("the exponent is a number");
    let (sign, mantissa) = mantissa
        .strip_prefix('-')
        .map_or(("", mantissa), |rest| ("-", rest));
    let digits: String = mantissa.chars().filter(|c| *c != '.').collect();
    let digits = digits.trim_end_matches('0');
    let digits = if digits.is_empty() { "0" } else { digits };

    if exponent < MIN_FIXED_EXPONENT || exponent >= i32::try_from(precision).unwrap_or(i32::MAX) {
        let (first, rest) = digits.split_at(1);
        let point = if rest.is_empty() { "" } else { "." };
        let exponent_sign = if exponent < 0 { '-' } else { '+' };
        format!(
            "{sign}{first}{point}{rest}e{exponent_sign}{:02}",
            exponent.abs()
        )
    } else if exponent >= 0 {
        let integer_length = exponent.unsigned_abs() as usize + 1;
        let padded = format!("{digits:0<integer_length$}");
        let (integer, fraction) = padded.split_at(integer_length);
        let point = if fraction.is_empty() { "" } else { "." };
        format!("{sign}{integer}{point}{fraction}")
    } else {
        let zeros = "0".repeat(exponent.unsigned_abs() as usize - 1);
        format!("{sign}0.{zeros}{digits}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn format_g_follows_the_c_conversion() {
        // Arrange
        let cases = [
            (0.0, 15, "0"),
            (-0.0, 15, "-0"),
            (1.5, 15, "1.5"),
            (100.0, 15, "100"),
            (1e15, 15, "1e+15"),
            (123456.0, 15, "123456"),
            (0.0001, 15, "0.0001"),
            (0.00001, 15, "1e-05"),
            (-2.5e-7, 15, "-2.5e-07"),
            (1e100, 15, "1e+100"),
            (123_456_789.123_456_79, 17, "123456789.12345679"),
        ];

        for (value, precision, expected) in cases {
            // Act
            let text = format_g(value, precision);

            // Assert
            assert_eq!(text, expected, "{value:e} at {precision}");
        }
    }
}
