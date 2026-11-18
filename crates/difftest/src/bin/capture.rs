//! Prints reference outputs as fixture text, and checks committed fixtures.
//!
//! ```text
//! capture <kind> <input file>   print a fixture for the inputs in the file
//! capture --check [path]        recompute a fixture file, or every committed one
//! ```
//!
//! # File format
//!
//! Both input files and fixture files are line based text. A file is a header,
//! then cases, each starting with a line `case`. A header or case line is one of:
//!
//! - `key: value`, a single line value. Bytes are lowercase hex, and numbers are decimal.
//! - `key:` followed by lines that start with `|`, a text value. Each such line is
//!   `| ` and a line of the text, or `|` alone for an empty line. The lines are
//!   joined by `\n`, so a text that ends with a newline ends with a `|` line.
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

use std::{
    env, fs,
    path::{Path, PathBuf},
    process::ExitCode,
};

use cpsat_difftest::{
    REFERENCE_COMMIT, reference_debug_string, reference_fasthash64, reference_parse_text,
    reference_version,
};

const USAGE: &str = "usage: capture <kind> <input file> | capture --check [path]";
const CHECK_FLAG: &str = "--check";
const CASE_LINE: &str = "case";
const TEXT_MARK: char = '|';
const COMMENT_MARK: char = '#';
const KEY_END: &str = ":";
const DEFAULT_LIMIT: i32 = i32::MAX;

/// Where committed fixture files live: `crates/*/tests/fixtures/*.txt`, as
/// (directory of the crates, directory in a crate, file extension).
const FIXTURE_LOCATION: (&str, &str, &str) = ("crates", "tests/fixtures", "txt");

type Case = Vec<Entry>;

#[derive(Debug, Clone, PartialEq)]
struct Entry {
    key: String,
    value: Value,
}

#[derive(Debug, Clone, PartialEq)]
enum Value {
    Line(String),
    Text(String),
}

impl Value {
    fn as_str(&self) -> &str {
        match self {
            Value::Line(text) | Value::Text(text) => text,
        }
    }
}

fn line(key: &str, value: impl ToString) -> Entry {
    Entry {
        key: key.to_owned(),
        value: Value::Line(value.to_string()),
    }
}

fn text(key: &str, value: impl Into<String>) -> Entry {
    Entry {
        key: key.to_owned(),
        value: Value::Text(value.into()),
    }
}

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

fn to_hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn from_hex(hex: &str) -> Result<Vec<u8>, String> {
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

fn single<'a>(case: &'a Case, key: &str) -> Result<&'a str, String> {
    let mut found = case.iter().filter(|entry| entry.key == key);
    match (found.next(), found.next()) {
        (Some(entry), None) => Ok(entry.value.as_str()),
        (None, _) => Err(format!("a case has no `{key}`")),
        (Some(_), Some(_)) => Err(format!("a case has more than one `{key}`")),
    }
}

fn all<'a>(case: &'a Case, key: &str) -> Vec<&'a str> {
    case.iter()
        .filter(|entry| entry.key == key)
        .map(|entry| entry.value.as_str())
        .collect()
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

fn render(kind: Kind, cases: &[Case]) -> String {
    let mut out = String::new();
    render_entry(&line("commit", REFERENCE_COMMIT), &mut out);
    render_entry(&line("kind", kind.name()), &mut out);
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
struct Document {
    header: Case,
    cases: Vec<Case>,
}

fn parse(source: &str) -> Result<Document, String> {
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
    Ok(render(kind, &recompute(kind, &document.cases)?))
}

fn check_text(source: &str) -> Result<(), String> {
    let document = parse(source)?;
    let kind_name = single(&document.header, "kind")?;
    let kind = Kind::parse(kind_name)?;
    let expected = render(kind, &recompute(kind, &document.cases)?);
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

fn directory_files(directory: &Path) -> Vec<PathBuf> {
    let Ok(entries) = fs::read_dir(directory) else {
        return Vec::new();
    };
    let mut paths: Vec<PathBuf> = entries
        .filter_map(|entry| Some(entry.ok()?.path()))
        .collect();
    paths.sort();
    paths
}

fn find_fixtures(root: &Path) -> Vec<PathBuf> {
    let (crates_dir, fixture_dir, extension) = FIXTURE_LOCATION;
    directory_files(&root.join(crates_dir))
        .into_iter()
        .flat_map(|crate_dir| directory_files(&crate_dir.join(fixture_dir)))
        .filter(|path| path.is_file() && path.extension().is_some_and(|found| found == extension))
        .collect()
}

fn repository_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
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

    const PARSE_INPUT: &str = "case\nmessage: operations_research.sat.CpModelProto\n\
                               text:\n| variables { domain: 0 domain: 1 }\n";

    fn temp_dir(name: &str) -> PathBuf {
        let dir = env::temp_dir().join(format!("cpsat-difftest-{}-{name}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir
    }

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
    fn text_values_round_trip_through_the_file_format() {
        // Arrange
        let cases = vec![vec![
            line("bytes", ""),
            text("long", "# first\n\n  indented\r\nlast\n"),
            text("empty", ""),
        ]];
        let rendered = render(Kind::Parsed, &cases);

        // Act
        let document = parse(&rendered).unwrap();

        // Assert
        assert_eq!(document.cases, cases);
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

    #[test]
    fn find_fixtures_reads_every_text_file_under_a_crate_fixture_folder() {
        // Arrange
        let root = temp_dir("find");
        let fixtures = root.join("crates/alpha/tests/fixtures");
        fs::create_dir_all(&fixtures).unwrap();
        fs::create_dir_all(root.join("crates/beta/tests")).unwrap();
        fs::write(fixtures.join("one.txt"), "").unwrap();
        fs::write(fixtures.join("notes.md"), "").unwrap();

        // Act
        let found = find_fixtures(&root);

        // Assert
        assert_eq!(found, vec![fixtures.join("one.txt")]);
        fs::remove_dir_all(root).unwrap();
    }
}
