//! Reads the fixture files that the capture tool writes.

use std::fs;

const CASE_LINE: &str = "case";
const KEY_END: char = ':';
const TEXT_MARK: char = '|';
const HEX_RADIX: u32 = 16;
const HEX_DIGITS_PER_BYTE: usize = 2;

/// One case of a fixture file: its `key: value` entries in file order.
pub struct Case {
    entries: Vec<(String, String)>,
}

impl Case {
    /// Returns every value of `key` in file order.
    pub fn all(&self, key: &str) -> Vec<&str> {
        self.entries
            .iter()
            .filter(|(name, _)| name == key)
            .map(|(_, value)| value.as_str())
            .collect()
    }

    /// Returns the only value of `key`, and panics when there is none or several.
    pub fn single(&self, key: &str) -> &str {
        match self.all(key).as_slice() {
            [value] => value,
            found => panic!("expected one `{key}` in the case, found {}", found.len()),
        }
    }
}

/// Reads the fixture file at `path` and returns its cases, and panics when the file
/// cannot be read or does not follow the format of the capture tool.
pub fn read(path: &str) -> Vec<Case> {
    let source =
        fs::read_to_string(path).unwrap_or_else(|error| panic!("cannot read {path}: {error}"));
    let mut cases: Vec<Case> = Vec::new();
    let mut lines = source.split('\n').peekable();
    while let Some(line) = lines.next() {
        if line == CASE_LINE {
            cases.push(Case {
                entries: Vec::new(),
            });
        } else if let Some((key, value)) = line.split_once(KEY_END) {
            let value = if value.is_empty() {
                let mut text_lines = Vec::new();
                while let Some(text_line) = lines.next_if(|next| next.starts_with(TEXT_MARK)) {
                    let content = &text_line[TEXT_MARK.len_utf8()..];
                    text_lines.push(content.strip_prefix(' ').unwrap_or(content).to_owned());
                }
                text_lines.join("\n")
            } else {
                value.strip_prefix(' ').unwrap_or(value).to_owned()
            };
            if let Some(case) = cases.last_mut() {
                case.entries.push((key.to_owned(), value));
            }
        }
    }
    cases
}

/// Decodes lowercase hex text into bytes, and panics on a character that is not hex.
pub fn from_hex(hex: &str) -> Vec<u8> {
    hex.as_bytes()
        .chunks(HEX_DIGITS_PER_BYTE)
        .map(|pair| {
            let digits = std::str::from_utf8(pair).expect("hex text is ASCII");
            u8::from_str_radix(digits, HEX_RADIX)
                .unwrap_or_else(|_| panic!("`{digits}` is not hex"))
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn read_keeps_the_indentation_and_the_empty_lines_of_a_text_value() {
        // Arrange
        let path = std::env::temp_dir().join(format!("cpsat-fixture-{}.txt", std::process::id()));
        fs::write(
            &path,
            "commit: abc\nkind: parsed\ncase\ntext:\n| a {\n|   b\n|\n| }\nbytes: 0a\nbytes: ff\n",
        )
        .unwrap();

        // Act
        let cases = read(path.to_str().unwrap());
        fs::remove_file(&path).unwrap();

        // Assert
        assert_eq!(cases.len(), 1);
        assert_eq!(cases[0].single("text"), "a {\n  b\n\n}");
        assert_eq!(cases[0].all("bytes"), ["0a", "ff"]);
    }

    #[test]
    fn read_keeps_a_carriage_return_and_the_leading_spaces_of_a_text_line() {
        // Arrange
        let path =
            std::env::temp_dir().join(format!("cpsat-fixture-cr-{}.txt", std::process::id()));
        fs::write(&path, "case\ntext:\n|   indented\r\n| last\n").unwrap();

        // Act
        let cases = read(path.to_str().unwrap());
        fs::remove_file(&path).unwrap();

        // Assert
        assert_eq!(cases[0].single("text"), "  indented\r\nlast");
    }

    #[test]
    fn from_hex_decodes_each_pair_of_digits() {
        // Arrange, Act
        let bytes = from_hex("007fff");

        // Assert
        assert_eq!(bytes, [0x00, 0x7f, 0xff]);
    }
}
