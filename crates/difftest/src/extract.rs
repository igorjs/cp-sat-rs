//! Reads the text blocks of a C++ test source.

/// The delimiters of the raw string that upstream tests write protobuf text in.
const BLOCK_START: &str = "R\"pb(";
const BLOCK_END: &str = ")pb\"";

/// The text of one `R"pb(...)pb"` raw string and the line its start is on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Block {
    pub line: usize,
    pub text: String,
}

/// Returns each `R"pb(...)pb"` block of the source in file order. Raw strings
/// with another delimiter are not blocks. A block that never ends is an error.
pub fn extract(source: &str) -> Result<Vec<Block>, String> {
    let mut blocks = Vec::new();
    let mut offset = 0;
    while let Some(found) = source[offset..].find(BLOCK_START) {
        let start = offset + found;
        let line = line_of(source, start);
        let text_start = start + BLOCK_START.len();
        let length = source[text_start..]
            .find(BLOCK_END)
            .ok_or_else(|| format!("the block that starts on line {line} never ends"))?;
        blocks.push(Block {
            line,
            text: source[text_start..text_start + length].to_owned(),
        });
        offset = text_start + length + BLOCK_END.len();
    }
    Ok(blocks)
}

fn line_of(source: &str, offset: usize) -> usize {
    source[..offset].matches('\n').count() + 1
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_the_two_pb_blocks_and_not_a_raw_string_of_another_kind() {
        // Arrange
        let source = "const char* a = R\"pb(variables { domain: 0 })pb\";\n\
                      const char* b = R\"(not a block)\";\n\
                      const char* c = R\"pb(\n  constraints {}\n)pb\";\n";

        // Act
        let blocks = extract(source).unwrap();

        // Assert
        assert_eq!(
            blocks,
            vec![
                Block {
                    line: 1,
                    text: "variables { domain: 0 }".to_owned()
                },
                Block {
                    line: 3,
                    text: "\n  constraints {}\n".to_owned()
                },
            ]
        );
    }

    #[test]
    fn returns_no_block_for_a_source_without_one() {
        // Arrange
        let source = "int main() { return 0; }\n";

        // Act
        let blocks = extract(source).unwrap();

        // Assert
        assert!(blocks.is_empty());
    }

    #[test]
    fn counts_the_lines_inside_an_earlier_block() {
        // Arrange
        let source = "R\"pb(\n\n)pb\"\n\nR\"pb(x)pb\"";

        // Act
        let blocks = extract(source).unwrap();

        // Assert
        assert_eq!(blocks[1].line, 5);
    }

    #[test]
    fn fails_for_a_block_that_never_ends() {
        // Arrange
        let source = "\n\nR\"pb(variables {";

        // Act
        let error = extract(source).unwrap_err();

        // Assert
        assert!(error.contains("line 3"), "{error}");
    }
}
