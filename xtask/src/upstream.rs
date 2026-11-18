//! Reads the upstream OR-Tools source: file sizes, include lines and test cases.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::Path;

const INCLUDE_PREFIX: &str = "#include \"ortools/";
const GENERATED_HEADER_SUFFIX: &str = ".pb.h";
pub const TEST_FILE_SUFFIX: &str = "_test.cc";
/// Files with this in their name are test code: the test files, and test helpers such as
/// `cp_model_test_utils.h`. The plan's totals leave all of them out.
const TEST_NAME_MARKER: &str = "_test";
const SOURCE_SUFFIX: &str = ".cc";
const HEADER_SUFFIX: &str = ".h";
const DEFINE_PREFIX: &str = "#define ";
const LINE_CONTINUATION: char = '\\';
/// `TEST_P` counts as a case: the plan's totals only add up with it.
const TEST_MACROS: [&str; 4] = ["TEST", "TEST_F", "TEST_P", "TYPED_TEST"];
pub const TYPED_TEST_MACRO: &str = "TYPED_TEST";
pub const SAT_FOLDER: &str = "sat";

/// One upstream file, with the includes that exist in the source tree.
#[derive(Debug)]
pub struct SourceFile {
    pub lines: usize,
    pub includes: BTreeSet<String>,
}

/// One test case written out in the test file, with the macro that makes it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Case {
    pub macro_name: &'static str,
    pub suite: String,
    pub name: String,
}

/// Every case of a test file, in file order. A use of a macro that the file defines around
/// a test is one case, named by the macro's test header with the use's arguments put in.
#[derive(Debug, Default)]
pub struct Cases {
    pub written: Vec<Case>,
}

impl Cases {
    pub fn total(&self) -> usize {
        self.written.len()
    }
}

/// The test header inside a macro the test file defines, with the macro's parameters.
#[derive(Debug)]
struct Wrapper {
    parameters: Vec<String>,
    macro_name: &'static str,
    suite: String,
    name: String,
}

impl Wrapper {
    fn case_for(&self, arguments: &[String]) -> Case {
        let put_in = |token: &str| {
            self.parameters
                .iter()
                .position(|parameter| parameter == token)
                .and_then(|at| arguments.get(at))
                .map_or_else(|| token.to_string(), Clone::clone)
        };
        Case {
            macro_name: self.macro_name,
            suite: put_in(&self.suite),
            name: put_in(&self.name),
        }
    }
}

pub fn line_count(text: &str) -> usize {
    text.lines().count()
}

/// Paths of the `.cc` and `.h` files directly in `sat/`, split into non-test files and
/// `_test.cc` files. Test helpers are in neither list.
pub fn sat_files(upstream: &Path) -> Result<(Vec<String>, Vec<String>), String> {
    let folder = upstream.join(SAT_FOLDER);
    let entries = fs::read_dir(&folder)
        .map_err(|error| format!("cannot read {}: {error}", folder.display()))?;
    let (mut sources, mut tests) = (Vec::new(), Vec::new());
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot read {}: {error}", folder.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = format!("{SAT_FOLDER}/{name}");
        if name.ends_with(TEST_FILE_SUFFIX) {
            tests.push(path);
        } else if !name.contains(TEST_NAME_MARKER)
            && (name.ends_with(SOURCE_SUFFIX) || name.ends_with(HEADER_SUFFIX))
        {
            sources.push(path);
        }
    }
    sources.sort();
    tests.sort();
    Ok((sources, tests))
}

/// The paths named by `#include "ortools/..."` lines, without the prefix.
/// Generated protobuf headers are left out.
pub fn include_paths(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|line| line.trim().strip_prefix(INCLUDE_PREFIX))
        .filter_map(|rest| rest.split('"').next())
        .filter(|path| !path.ends_with(GENERATED_HEADER_SUFFIX))
        .map(str::to_string)
        .collect()
}

fn sibling(path: &str) -> Option<String> {
    if let Some(stem) = path.strip_suffix(HEADER_SUFFIX) {
        Some(format!("{stem}{SOURCE_SUFFIX}"))
    } else {
        path.strip_suffix(SOURCE_SUFFIX)
            .map(|stem| format!("{stem}{HEADER_SUFFIX}"))
    }
}

/// Every file reachable from `start` through include lines. A header also pulls in the
/// `.cc` of the same name, and the other way round, since that file holds its includes.
pub fn include_closure(
    upstream: &Path,
    start: &[String],
) -> Result<BTreeMap<String, SourceFile>, String> {
    let mut found = BTreeMap::new();
    let mut queue: VecDeque<String> = start.iter().cloned().collect();
    while let Some(path) = queue.pop_front() {
        if found.contains_key(&path) {
            continue;
        }
        let text = fs::read_to_string(upstream.join(&path))
            .map_err(|error| format!("cannot read upstream file {path}: {error}"))?;
        let includes: BTreeSet<String> = include_paths(&text)
            .into_iter()
            .filter(|include| upstream.join(include).is_file())
            .collect();
        queue.extend(includes.iter().cloned());
        queue.extend(sibling(&path).filter(|other| upstream.join(other).is_file()));
        found.insert(
            path,
            SourceFile {
                lines: line_count(&text),
                includes,
            },
        );
    }
    Ok(found)
}

/// The test file that sits beside `path` and carries its name, if the source tree has one.
pub fn test_file_for(upstream: &Path, path: &str) -> Option<String> {
    let stem = path
        .strip_suffix(SOURCE_SUFFIX)
        .or_else(|| path.strip_suffix(HEADER_SUFFIX))?;
    let candidate = format!("{stem}{TEST_FILE_SUFFIX}");
    upstream.join(&candidate).is_file().then_some(candidate)
}

fn is_word_char(character: char) -> bool {
    character.is_ascii_alphanumeric() || character == '_'
}

/// The text of a call up to its closing parenthesis, given the text after the opening one.
/// `None` when the call goes on past the end of the text.
fn call_arguments(after_open: &str) -> Option<&str> {
    let (mut depth, mut quoted) = (1_usize, false);
    for (at, character) in after_open.char_indices() {
        match character {
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => {
                depth -= 1;
                if depth == 0 {
                    return Some(&after_open[..at]);
                }
            }
            _ => {}
        }
    }
    None
}

/// The arguments of a call that starts at `lines[0]`, which may go on over later lines.
fn call_arguments_over_lines(after_open: &str, later: &[&str]) -> Vec<String> {
    let mut text = after_open.to_string();
    let mut rest = later.iter();
    loop {
        if let Some(inside) = call_arguments(&text) {
            return split_arguments(inside);
        }
        match rest.next() {
            Some(line) => {
                text.push(' ');
                text.push_str(line.trim());
            }
            None => return split_arguments(&text),
        }
    }
}

/// Splits on the commas outside parentheses and quotes.
fn split_arguments(text: &str) -> Vec<String> {
    let mut arguments = Vec::new();
    let (mut depth, mut quoted, mut start) = (0_usize, false, 0);
    for (at, character) in text.char_indices() {
        match character {
            '"' => quoted = !quoted,
            '(' if !quoted => depth += 1,
            ')' if !quoted => depth = depth.saturating_sub(1),
            ',' if !quoted && depth == 0 => {
                arguments.push(text[start..at].trim().to_string());
                start = at + 1;
            }
            _ => {}
        }
    }
    arguments.push(text[start..].trim().to_string());
    arguments
}

fn test_macro_at_start(line: &str) -> Option<(&'static str, &str)> {
    TEST_MACROS.iter().find_map(|name| {
        line.strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('('))
            .map(|arguments| (*name, arguments))
    })
}

/// The first test macro call in `text` that is not the tail of a longer name.
fn find_test_call(text: &str) -> Option<(&'static str, &str)> {
    let mut found: Option<(usize, &'static str)> = None;
    for name in TEST_MACROS {
        let call = format!("{name}(");
        let at = text.match_indices(&call).map(|(at, _)| at).find(|&at| {
            text[..at]
                .chars()
                .next_back()
                .is_none_or(|before| !is_word_char(before))
        });
        if let Some(at) = at
            && found.is_none_or(|(best, _)| at < best)
        {
            found = Some((at, name));
        }
    }
    let (at, name) = found?;
    Some((name, &text[at + name.len() + 1..]))
}

/// Reads a `#define NAME(parameters) body` that makes a test: `None` for any other define.
fn parse_wrapper(define: &str) -> Option<(String, Wrapper)> {
    let rest = define.trim().strip_prefix(DEFINE_PREFIX)?.trim_start();
    let name_end = rest.find(|character: char| !is_word_char(character))?;
    let after_name = rest[name_end..].strip_prefix('(')?;
    let parameters = call_arguments(after_name)?;
    let body = &after_name[parameters.len() + 1..];
    let (macro_name, after_open) = find_test_call(body)?;
    let mut header = split_arguments(call_arguments(after_open)?).into_iter();
    Some((
        rest[..name_end].to_string(),
        Wrapper {
            parameters: split_arguments(parameters),
            macro_name,
            suite: header.next().unwrap_or_default(),
            name: header.next().unwrap_or_default(),
        },
    ))
}

/// Splits the file into the lines outside `#define` bodies and the macros whose body
/// makes a test.
fn split_defines(text: &str) -> (Vec<&str>, BTreeMap<String, Wrapper>) {
    let mut outside = Vec::new();
    let mut wrappers = BTreeMap::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if !line.trim().starts_with(DEFINE_PREFIX) {
            outside.push(line);
            continue;
        }
        let mut define = vec![line];
        while define
            .last()
            .is_some_and(|last| last.trim_end().ends_with(LINE_CONTINUATION))
        {
            let Some(next) = lines.next() else { break };
            define.push(next);
        }
        let joined = define
            .iter()
            .map(|part| part.trim().trim_end_matches(LINE_CONTINUATION))
            .collect::<Vec<_>>()
            .join(" ");
        if let Some((name, wrapper)) = parse_wrapper(&joined) {
            wrappers.insert(name, wrapper);
        }
    }
    (outside, wrappers)
}

/// Counts the test cases of one upstream test file: the `TEST`, `TEST_F`, `TEST_P` and
/// `TYPED_TEST` lines outside `#define` bodies, and one case for each use of a macro
/// that defines a test around its arguments. A header or a use may span several lines.
pub fn count_cases(text: &str) -> Cases {
    let (outside, wrappers) = split_defines(text);
    let mut cases = Cases::default();
    for (at, line) in outside.iter().enumerate() {
        let line = line.trim();
        let later = &outside[at + 1..];
        if let Some((macro_name, after_open)) = test_macro_at_start(line) {
            let mut arguments = call_arguments_over_lines(after_open, later).into_iter();
            cases.written.push(Case {
                macro_name,
                suite: arguments.next().unwrap_or_default(),
                name: arguments.next().unwrap_or_default(),
            });
        } else if let Some((wrapper, after_open)) = wrappers.iter().find_map(|(name, wrapper)| {
            let after_open = line.strip_prefix(name.as_str())?.strip_prefix('(')?;
            Some((wrapper, after_open))
        }) {
            let arguments = call_arguments_over_lines(after_open, later);
            cases.written.push(wrapper.case_for(&arguments));
        }
    }
    cases
}
