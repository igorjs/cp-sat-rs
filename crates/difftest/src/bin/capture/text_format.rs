//! The line based text format of input and fixture files.
//!
//! A file is a header, then cases, each starting with a line `case`. A header or
//! case line is one of:
//!
//! - `key: value`, a single line value.
//! - `key:` followed by lines that start with `|`, a text value. Each such line is
//!   `| ` and a line of the text, or `|` alone for an empty line. The lines are
//!   joined by `\n`, so a text that ends with a newline ends with a `|` line.
//!
//! Blank lines and lines that start with `#` are ignored when parsing.

const CASE_LINE: &str = "case";
const TEXT_MARK: char = '|';
const COMMENT_MARK: char = '#';
const KEY_END: &str = ":";

pub type Case = Vec<Entry>;

#[derive(Debug, Clone, PartialEq)]
pub struct Entry {
    pub key: String,
    pub value: Value,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Value {
    Line(String),
    Text(String),
}

impl Value {
    pub fn as_str(&self) -> &str {
        match self {
            Value::Line(text) | Value::Text(text) => text,
        }
    }
}

pub fn line(key: &str, value: impl ToString) -> Entry {
    Entry {
        key: key.to_owned(),
        value: Value::Line(value.to_string()),
    }
}

pub fn text(key: &str, value: impl Into<String>) -> Entry {
    Entry {
        key: key.to_owned(),
        value: Value::Text(value.into()),
    }
}

pub fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

pub fn from_hex(hex: &str) -> Result<Vec<u8>, String> {
    let digit = |byte: u8| {
        char::from(byte)
            .to_digit(16)
            .and_then(|value| u8::try_from(value).ok())
    };
    hex.as_bytes()
        .chunks(2)
        .map(|pair| match *pair {
            [high, low] => digit(high)
                .zip(digit(low))
                .map(|(high, low)| high << 4 | low)
                .ok_or_else(|| format!("`{hex}` holds a character that is not a hex digit")),
            _ => Err(format!("`{hex}` is not an even number of hex digits")),
        })
        .collect()
}

pub fn single<'a>(case: &'a Case, key: &str) -> Result<&'a str, String> {
    let mut found = case.iter().filter(|entry| entry.key == key);
    match (found.next(), found.next()) {
        (Some(entry), None) => Ok(entry.value.as_str()),
        (None, _) => Err(format!("a case has no `{key}`")),
        (Some(_), Some(_)) => Err(format!("a case has more than one `{key}`")),
    }
}

pub fn all<'a>(case: &'a Case, key: &str) -> Vec<&'a str> {
    case.iter()
        .filter(|entry| entry.key == key)
        .map(|entry| entry.value.as_str())
        .collect()
}

fn render_entry(entry: &Entry, out: &mut String) {
    match &entry.value {
        Value::Line(value) if value.is_empty() => {
            out.push_str(&format!("{}{KEY_END}\n", entry.key))
        }
        Value::Line(value) => out.push_str(&format!("{}{KEY_END} {value}\n", entry.key)),
        Value::Text(value) => {
            out.push_str(&format!("{}{KEY_END}\n", entry.key));
            for text_line in value.split('\n') {
                if text_line.is_empty() {
                    out.push(TEXT_MARK);
                } else {
                    out.push_str(&format!("{TEXT_MARK} {text_line}"));
                }
                out.push('\n');
            }
        }
    }
}

/// Renders the header entries, then the cases.
pub fn render(header: &[Entry], cases: &[Case]) -> String {
    let mut out = String::new();
    for entry in header {
        render_entry(entry, &mut out);
    }
    for case in cases {
        out.push_str(CASE_LINE);
        out.push('\n');
        for entry in case {
            render_entry(entry, &mut out);
        }
    }
    out
}

/// A parsed file: the entries before the first `case` line, then the cases.
#[derive(Debug)]
pub struct Document {
    pub header: Case,
    pub cases: Vec<Case>,
}

pub fn parse(source: &str) -> Result<Document, String> {
    let mut header = Case::new();
    let mut cases: Vec<Case> = Vec::new();
    let mut open_text = false;
    for (index, raw) in source.split('\n').enumerate() {
        let number = index + 1;
        let fail = |message: &str| format!("line {number}: {message}");
        if let Some(rest) = raw.strip_prefix(TEXT_MARK) {
            let content = match rest.strip_prefix(' ') {
                Some(content) => content,
                None if rest.is_empty() => "",
                None => return Err(fail("a text line must be `|` or `| ` and text")),
            };
            let current = cases.last_mut().unwrap_or(&mut header);
            match current.last_mut() {
                Some(Entry {
                    value: Value::Text(existing),
                    ..
                }) if open_text => {
                    existing.push('\n');
                    existing.push_str(content);
                }
                Some(entry) if !open_text && entry.value == Value::Line(String::new()) => {
                    entry.value = Value::Text(content.to_owned());
                    open_text = true;
                }
                _ => return Err(fail("a text line must follow a `key:` line")),
            }
            continue;
        }
        open_text = false;
        if raw.is_empty() || raw.starts_with(COMMENT_MARK) {
            continue;
        }
        if raw == CASE_LINE {
            cases.push(Case::new());
            continue;
        }
        let (key, value) = match raw.split_once(KEY_END) {
            Some((key, "")) => (key, ""),
            Some((key, value)) => (
                key,
                value
                    .strip_prefix(' ')
                    .ok_or_else(|| fail("a value must follow `key:` and a space"))?,
            ),
            None => return Err(fail("expected `key: value`, `case` or a text line")),
        };
        if key.is_empty() || key.contains(char::is_whitespace) {
            return Err(fail("a key is one word"));
        }
        cases
            .last_mut()
            .unwrap_or(&mut header)
            .push(line(key, value));
    }
    Ok(Document { header, cases })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_values_round_trip_through_the_file_format() {
        // Arrange
        let cases = vec![vec![
            line("bytes", ""),
            text("long", "# first\n\n  indented\r\nlast\n"),
            text("empty", ""),
        ]];
        let rendered = render(&[line("kind", "parsed")], &cases);

        // Act
        let document = parse(&rendered).unwrap();

        // Assert
        assert_eq!(document.header, vec![line("kind", "parsed")]);
        assert_eq!(document.cases, cases);
    }

    #[test]
    fn parse_reports_the_line_of_a_text_line_with_no_key() {
        // Arrange
        let source = "case\n| orphan\n";

        // Act
        let error = parse(source).unwrap_err();

        // Assert
        assert!(error.starts_with("line 2:"), "{error}");
    }

    #[test]
    fn hex_round_trips_and_rejects_bad_input() {
        // Arrange
        let bytes = [0x00, 0x7f, 0xff];

        // Act
        let hex = to_hex(&bytes);

        // Assert
        assert_eq!(hex, "007fff");
        assert_eq!(from_hex(&hex), Ok(bytes.to_vec()));
        assert!(from_hex("abc").is_err());
        assert!(from_hex("zz").is_err());
    }
}
