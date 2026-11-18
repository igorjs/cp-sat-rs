//! Reads the upstream OR-Tools source: file sizes, include lines and test cases.

use std::collections::{BTreeMap, BTreeSet, VecDeque};
use std::fs;
use std::path::Path;

const INCLUDE_PREFIX: &str = "#include \"ortools/";
const GENERATED_HEADER_SUFFIX: &str = ".pb.h";
const TEST_FILE_SUFFIX: &str = "_test.cc";
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

#[derive(Debug, Default)]
pub struct Cases {
    pub written: Vec<Case>,
    /// Uses of a macro that defines a test around its arguments.
    pub macro_uses: usize,
}

impl Cases {
    pub fn total(&self) -> usize {
        self.written.len() + self.macro_uses
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

fn test_macro_at_start(line: &str) -> Option<(&'static str, &str)> {
    TEST_MACROS.iter().find_map(|name| {
        line.strip_prefix(name)
            .and_then(|rest| rest.strip_prefix('('))
            .map(|arguments| (*name, arguments))
    })
}

fn invokes_test_macro(line: &str) -> bool {
    TEST_MACROS.iter().any(|name| {
        let call = format!("{name}(");
        line.match_indices(&call).any(|(at, _)| {
            line[..at]
                .chars()
                .next_back()
                .is_none_or(|before| !is_word_char(before))
        })
    })
}

fn macro_name(define_line: &str) -> Option<&str> {
    let rest = define_line.trim().strip_prefix(DEFINE_PREFIX)?.trim_start();
    let end = rest.find(|character: char| !is_word_char(character))?;
    (rest[end..].starts_with('(') && end > 0).then(|| &rest[..end])
}

/// Splits the file into the lines outside `#define` bodies and the names of the macros
/// whose body makes a test.
fn split_defines(text: &str) -> (Vec<&str>, BTreeSet<String>) {
    let mut outside = Vec::new();
    let mut wrappers = BTreeSet::new();
    let mut open: Option<(Option<&str>, bool)> = None;
    for line in text.lines() {
        let (name, mut makes_test) = match open.take() {
            Some(state) => state,
            None if line.trim().starts_with(DEFINE_PREFIX) => (macro_name(line), false),
            None => {
                outside.push(line);
                continue;
            }
        };
        makes_test |= invokes_test_macro(line);
        if line.trim_end().ends_with(LINE_CONTINUATION) {
            open = Some((name, makes_test));
        } else if let (Some(name), true) = (name, makes_test) {
            wrappers.insert(name.to_string());
        }
    }
    (outside, wrappers)
}

fn parse_case(macro_name: &'static str, arguments: &str) -> Case {
    let inside = arguments.split(')').next().unwrap_or_default();
    let mut parts = inside.split(',').map(|part| part.trim().to_string());
    Case {
        macro_name,
        suite: parts.next().unwrap_or_default(),
        name: parts.next().unwrap_or_default(),
    }
}

/// Counts the test cases of one upstream test file: the `TEST`, `TEST_F`, `TEST_P` and
/// `TYPED_TEST` lines outside `#define` bodies, and one case for each use of a macro
/// that defines a test around its arguments.
pub fn count_cases(text: &str) -> Cases {
    let (outside, wrappers) = split_defines(text);
    let mut cases = Cases::default();
    for line in outside {
        let line = line.trim();
        if let Some((macro_name, arguments)) = test_macro_at_start(line) {
            cases.written.push(parse_case(macro_name, arguments));
        } else if wrappers.iter().any(|name| {
            line.strip_prefix(name.as_str())
                .is_some_and(|rest| rest.starts_with('('))
        }) {
            cases.macro_uses += 1;
        }
    }
    cases
}
