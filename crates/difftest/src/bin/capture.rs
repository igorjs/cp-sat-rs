//! Prints reference outputs as fixture text, and checks committed fixtures.
//!
//! ```text
//! capture <kind> <input file>   print a fixture for the inputs in the file
//! capture --check [path]        recompute a fixture file, or every committed one
//! ```
//!
//! # File format
//!
//! Both input files and fixture files use the line based text format of
//! `text_format`. Bytes are lowercase hex, and numbers are decimal.
//!
//! In an input file, blank lines and lines that start with `#` are ignored and the
//! header is empty. A fixture file starts with the header `commit` and `kind`,
//! holds no comment, and holds for each case its inputs followed by its outputs.
//!
//! | kind | inputs | outputs |
//! |---|---|---|
//! | `debug_strings` | `message`, `bytes`, `limits` (`max_chars max_lines`, repeated) | `short`, `long`, `limited` (one for each `limits`) |
//! | `parsed` | `message`, `text` | `bytes` |
//! | `version` | none, and the file has exactly one case | `version` |
//! | `hashes` | `bytes`, `seed` | `hash` |
//!
//! `message` is a full name such as `operations_research.sat.CpModelProto`. `long`
//! uses the default limits of `ProtobufDebugString`.
//!
//! # Input files
//!
//! A committed input file is `crates/*/tests/fixtures/inputs/<kind>.<name>.in`, and
//! the text before the first dot is its kind. The CI job `reference` prints
//! `capture <kind> <file>` for each of them on a manual run, so a fixture can be
//! made there. The fixture is the output, saved as `crates/*/tests/fixtures/<name>.txt`.
//! `--check` reads only files with the fixture extension directly in the fixture
//! folder, so an input file is never taken for a fixture.

#[path = "capture/locations.rs"]
mod locations;
#[path = "capture/text_format.rs"]
mod text_format;

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use cpsat_difftest::{
    REFERENCE_COMMIT, reference_debug_string, reference_fasthash64, reference_parse_text,
    reference_version,
};
use locations::{find_fixtures, repository_root};
use text_format::{Case, all, from_hex, line, parse, render, single, text, to_hex};

const USAGE: &str = "usage: capture <kind> <input file> | capture --check [path]";
const CHECK_FLAG: &str = "--check";
const DEFAULT_LIMIT: i32 = i32::MAX;

#[derive(Debug, Clone, Copy, PartialEq)]
enum Kind {
    DebugStrings,
    Parsed,
    Version,
    Hashes,
}

impl Kind {
    const ALL: [(Kind, &'static str); 4] = [
        (Kind::DebugStrings, "debug_strings"),
        (Kind::Parsed, "parsed"),
        (Kind::Version, "version"),
        (Kind::Hashes, "hashes"),
    ];

    fn parse(name: &str) -> Result<Kind, String> {
        Self::ALL
            .iter()
            .find(|(_, known)| *known == name)
            .map(|(kind, _)| *kind)
            .ok_or_else(|| {
                let names: Vec<&str> = Self::ALL.iter().map(|(_, known)| *known).collect();
                format!(
                    "unknown kind `{name}`, expected one of {}",
                    names.join(", ")
                )
            })
    }

    fn name(self) -> &'static str {
        Self::ALL
            .iter()
            .find(|(kind, _)| *kind == self)
            .map_or("", |(_, name)| name)
    }

    fn input_keys(self) -> &'static [&'static str] {
        match self {
            Kind::DebugStrings => &["message", "bytes", "limits"],
            Kind::Parsed => &["message", "text"],
            Kind::Version => &[],
            Kind::Hashes => &["bytes", "seed"],
        }
    }
}

fn parse_limits(pair: &str) -> Result<(i32, i32), String> {
    let mut numbers = pair.split_whitespace().map(str::parse::<i32>);
    match (numbers.next(), numbers.next(), numbers.next()) {
        (Some(Ok(max_chars)), Some(Ok(max_lines)), None) => Ok((max_chars, max_lines)),
        _ => Err(format!("limits `{pair}` is not `max_chars max_lines`")),
    }
}

/// Returns the case with its outputs, computed from the input entries only.
fn compute(kind: Kind, case: &Case) -> Result<Case, String> {
    let reference = |error: cxx::Exception| format!("the reference solver failed: {error}");
    match kind {
        Kind::DebugStrings => {
            let message = single(case, "message")?;
            let bytes = from_hex(single(case, "bytes")?)?;
            let limits = all(case, "limits");
            let mut out = vec![line("message", message), line("bytes", to_hex(&bytes))];
            out.extend(limits.iter().map(|pair| line("limits", pair)));
            let print = |short, max_chars, max_lines| {
                reference_debug_string(message, &bytes, short, max_chars, max_lines)
                    .map_err(reference)
            };
            out.push(text("short", print(true, DEFAULT_LIMIT, DEFAULT_LIMIT)?));
            out.push(text("long", print(false, DEFAULT_LIMIT, DEFAULT_LIMIT)?));
            for pair in limits {
                let (max_chars, max_lines) = parse_limits(pair)?;
                out.push(text("limited", print(false, max_chars, max_lines)?));
            }
            Ok(out)
        }
        Kind::Parsed => {
            let (message, source) = (single(case, "message")?, single(case, "text")?);
            let bytes = reference_parse_text(message, source).map_err(reference)?;
            Ok(vec![
                line("message", message),
                text("text", source),
                line("bytes", to_hex(&bytes)),
            ])
        }
        Kind::Version => Ok(vec![text(
            "version",
            reference_version().map_err(reference)?,
        )]),
        Kind::Hashes => {
            let bytes = from_hex(single(case, "bytes")?)?;
            let seed: u64 = single(case, "seed")?
                .parse()
                .map_err(|_| "`seed` is not an unsigned number".to_owned())?;
            let hash = reference_fasthash64(&bytes, seed).map_err(reference)?;
            Ok(vec![
                line("bytes", to_hex(&bytes)),
                line("seed", seed),
                line("hash", hash),
            ])
        }
    }
}

fn recompute(kind: Kind, cases: &[Case]) -> Result<Vec<Case>, String> {
    if kind == Kind::Version {
        return Ok(vec![compute(kind, &Case::new())?]);
    }
    cases.iter().map(|case| compute(kind, case)).collect()
}

fn render_fixture(kind: Kind, cases: &[Case]) -> String {
    render(
        &[line("commit", REFERENCE_COMMIT), line("kind", kind.name())],
        cases,
    )
}

fn capture_text(kind: Kind, input: &str) -> Result<String, String> {
    let document = parse(input)?;
    if !document.header.is_empty() {
        return Err("an input file holds no line before its first `case`".to_owned());
    }
    for entry in document.cases.iter().flatten() {
        if !kind.input_keys().contains(&entry.key.as_str()) {
            return Err(format!(
                "`{}` is not an input of `{}`",
                entry.key,
                kind.name()
            ));
        }
    }
    Ok(render_fixture(kind, &recompute(kind, &document.cases)?))
}

fn check_text(source: &str) -> Result<(), String> {
    let document = parse(source)?;
    let kind_name = single(&document.header, "kind")?;
    let kind = Kind::parse(kind_name)?;
    let expected = render_fixture(kind, &recompute(kind, &document.cases)?);
    if expected == source {
        return Ok(());
    }
    let mut file_lines = source.split('\n');
    let mut reference_lines = expected.split('\n');
    for number in 1.. {
        let (found, wanted) = (file_lines.next(), reference_lines.next());
        if found != wanted {
            return Err(format!(
                "line {number} differs: the file has `{}`, the reference gives `{}`",
                found.unwrap_or("<end of file>"),
                wanted.unwrap_or("<end of file>")
            ));
        }
    }
    unreachable!("the loop returns at the first differing line")
}

fn check_file(path: &Path) -> Result<(), String> {
    let source = fs::read_to_string(path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    check_text(&source).map_err(|error| format!("{}: {error}", path.display()))
}

fn run(args: &[String]) -> Result<String, String> {
    match args {
        [flag, rest @ ..] if flag == CHECK_FLAG && rest.len() <= 1 => {
            let paths = match rest {
                [path] => vec![PathBuf::from(path)],
                _ => find_fixtures(&repository_root()),
            };
            let mut report = String::new();
            for path in &paths {
                check_file(path)?;
                report.push_str(&format!("ok: {}\n", path.display()));
            }
            report.push_str(&format!(
                "checked {} fixture file(s) against {REFERENCE_COMMIT}\n",
                paths.len()
            ));
            Ok(report)
        }
        [kind, input_path] => {
            let source = fs::read_to_string(input_path)
                .map_err(|error| format!("cannot read {input_path}: {error}"))?;
            capture_text(Kind::parse(kind)?, &source)
        }
        _ => Err(USAGE.to_owned()),
    }
}

fn main() -> ExitCode {
    let args: Vec<String> = env::args().skip(1).collect();
    match run(&args) {
        Ok(output) => {
            print!("{output}");
            ExitCode::SUCCESS
        }
        Err(message) => {
            eprintln!("error: {message}");
            ExitCode::FAILURE
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::locations::temp_dir;

    const PARSE_INPUT: &str = "case\nmessage: operations_research.sat.CpModelProto\n\
                               text:\n| variables { domain: 0 domain: 1 }\n";

    /// Writes a fixture made by `capture`, changes the byte after `needle` and
    /// checks the file.
    fn check_after_changing_byte(name: &str, replacement: u8) -> Result<(), String> {
        let dir = temp_dir(name);
        let path = dir.join("parsed.txt");
        let fixture = capture_text(Kind::Parsed, PARSE_INPUT).unwrap();
        fs::write(&path, &fixture).unwrap();
        assert_eq!(
            run(&[CHECK_FLAG.to_owned(), path.display().to_string()]).map(drop),
            Ok(())
        );
        let needle = "bytes: ";
        let at = fixture.find(needle).unwrap() + needle.len();
        let mut changed = fixture.into_bytes();
        assert_ne!(changed[at], replacement);
        changed[at] = replacement;
        fs::write(&path, changed).unwrap();
        let result = run(&[CHECK_FLAG.to_owned(), path.display().to_string()]).map(drop);
        fs::remove_dir_all(dir).unwrap();
        result
    }

    #[test]
    fn check_fails_when_a_hex_digit_of_a_fixture_changes() {
        // Arrange, Act
        let result = check_after_changing_byte("digit", b'f');

        // Assert
        let error = result.unwrap_err();
        assert!(error.contains("differs"), "{error}");
    }

    #[test]
    fn check_fails_when_a_byte_of_a_fixture_is_not_hex() {
        // Arrange, Act
        let result = check_after_changing_byte("not-hex", b'z');

        // Assert
        assert!(result.is_err());
    }

    #[test]
    fn check_fails_for_a_fixture_from_another_commit() {
        // Arrange
        let fixture = capture_text(Kind::Version, "").unwrap();
        let other = fixture.replacen(REFERENCE_COMMIT, "0000", 1);

        // Act
        let error = check_text(&other).unwrap_err();

        // Assert
        assert!(error.contains("line 1 differs"), "{error}");
    }

    #[test]
    fn capture_rejects_a_key_that_is_not_an_input() {
        // Arrange
        let input = "case\nlimit: 1 1\n";

        // Act
        let error = capture_text(Kind::DebugStrings, input).unwrap_err();

        // Assert
        assert!(error.contains("`limit` is not an input"), "{error}");
    }

    #[test]
    fn debug_strings_hold_one_limited_text_for_each_limit_pair() {
        // Arrange
        let input = "case\nmessage: operations_research.sat.CpModelProto\nbytes: \n\
                     limits: 1000 10\nlimits: 0 10\n";

        // Act
        let fixture = capture_text(Kind::DebugStrings, input).unwrap();

        // Assert
        let document = parse(&fixture).unwrap();
        assert_eq!(all(&document.cases[0], "limited").len(), 2);
        assert_eq!(check_text(&fixture), Ok(()));
    }

    #[test]
    fn hashes_record_the_seed_and_the_hash() {
        // Arrange
        let input = "case\nbytes: 6370\nseed: 7\n";

        // Act
        let fixture = capture_text(Kind::Hashes, input).unwrap();

        // Assert
        assert!(fixture.contains("\nseed: 7\nhash: "), "{fixture}");
        assert_eq!(check_text(&fixture), Ok(()));
    }
}
