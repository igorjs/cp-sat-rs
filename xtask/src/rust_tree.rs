//! Reads the Rust sources of the repo: ported headers, stubs, ignored tests and pending lines.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

const PORTED_HEADER_PREFIX: &str = "// Ported from ortools/";
const STUB_MARKER: &str = "unimplemented!(\"phase";
const TEST_ATTRIBUTE: &str = "#[test]";
const ATTRIBUTE_START: &str = "#[";
const IGNORE_ATTRIBUTE: &str = "#[ignore";
const CFG_ATTR_PREFIX: &str = "#[cfg_attr(";
const IGNORE_WORD: &str = "ignore";
const PENDING_PREFIX: &str = "// PENDING(phase ";
const PENDING_MARKER: &str = "// PENDING(";
const PHASE_REASON_PREFIX: &str = "phase ";
const FUNCTION_KEYWORD: &str = "fn ";
/// How many lines below an attribute the function it belongs to may start.
const FUNCTION_LOOKAHEAD: usize = 8;
const SKIPPED_FOLDERS: [&str; 4] = ["target", ".git", "upstream", ".reference"];
const FIXTURES_FOLDER: &str = "xtask/tests/fixtures";

#[derive(Debug, Clone)]
pub struct IgnoredTest {
    pub file: String,
    pub line: usize,
    pub name: String,
    pub reason: Option<String>,
}

impl IgnoredTest {
    /// The phase the reason names, when it starts with `phase N:`.
    pub fn phase(&self) -> Option<u32> {
        let rest = self.reason.as_deref()?.strip_prefix(PHASE_REASON_PREFIX)?;
        let (number, _) = rest.split_once(':')?;
        number.parse().ok()
    }
}

/// A line standing in for a test that cannot compile yet.
#[derive(Debug, Clone)]
pub struct PendingLine {
    pub file: String,
    pub line: usize,
    pub phase: u32,
    /// The upstream case the line stands for, as `(suite, name)`.
    pub case: Option<(String, String)>,
}

#[derive(Debug, Default)]
pub struct RustTree {
    /// Upstream path without its extension, to whether the module holding the header
    /// has a stub.
    pub ported: BTreeMap<String, bool>,
    pub ignored: Vec<IgnoredTest>,
    pub pending: Vec<PendingLine>,
    /// Ported files that hold at least one test or pending line.
    pub test_holders: Vec<String>,
}

pub fn scan(root: &Path) -> Result<RustTree, String> {
    let mut tree = RustTree::default();
    visit(root, "", &mut tree)?;
    tree.ignored
        .sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    tree.pending
        .sort_by(|a, b| (&a.file, a.line).cmp(&(&b.file, b.line)));
    Ok(tree)
}

fn visit(folder: &Path, relative: &str, tree: &mut RustTree) -> Result<(), String> {
    let entries = fs::read_dir(folder)
        .map_err(|error| format!("cannot read {}: {error}", folder.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("cannot read {}: {error}", folder.display()))?;
        let name = entry.file_name().to_string_lossy().into_owned();
        let path = if relative.is_empty() {
            name.clone()
        } else {
            format!("{relative}/{name}")
        };
        let kind = entry
            .file_type()
            .map_err(|error| format!("cannot read {path}: {error}"))?;
        if kind.is_dir() {
            if !SKIPPED_FOLDERS.contains(&name.as_str()) && path != FIXTURES_FOLDER {
                visit(&entry.path(), &path, tree)?;
            }
        } else if kind.is_file() && name.ends_with(".rs") {
            let text = fs::read_to_string(entry.path())
                .map_err(|error| format!("cannot read {path}: {error}"))?;
            scan_file(&path, &text, tree);
        }
    }
    Ok(())
}

fn scan_file(path: &str, text: &str, tree: &mut RustTree) {
    let lines: Vec<&str> = text.lines().collect();
    // A file that still waits on a later phase, by a stub or a pending line, is partial.
    let has_stub = text.contains(STUB_MARKER)
        || lines
            .iter()
            .any(|line| line.trim().starts_with(PENDING_MARKER));
    let mut is_ported = false;
    let mut has_tests = false;
    for (at, line) in lines.iter().enumerate() {
        let trimmed = line.trim();
        if let Some(rest) = trimmed.strip_prefix(PORTED_HEADER_PREFIX) {
            let upstream_path = rest.split_whitespace().next().unwrap_or_default();
            let stem = upstream_path
                .rsplit_once('.')
                .map_or(upstream_path, |(stem, _)| stem);
            let entry = tree.ported.entry(stem.to_string()).or_insert(false);
            *entry |= has_stub;
            is_ported = true;
        } else if let Some(rest) = trimmed.strip_prefix(PENDING_PREFIX) {
            if let Some(pending) = parse_pending(path, at + 1, rest) {
                tree.pending.push(pending);
                has_tests = true;
            }
        } else if !trimmed.starts_with("//") {
            has_tests |= trimmed.contains(TEST_ATTRIBUTE);
            if let Some((reason, attribute_end)) = parse_ignore(trimmed) {
                tree.ignored.push(IgnoredTest {
                    file: path.to_string(),
                    line: at + 1,
                    name: function_after(&trimmed[attribute_end..], &lines[at + 1..]),
                    reason,
                });
            }
        }
    }
    if is_ported && has_tests {
        tree.test_holders.push(path.to_string());
    }
}

fn parse_pending(file: &str, line: usize, rest: &str) -> Option<PendingLine> {
    let (number, after) = rest.split_once(')')?;
    let case = after
        .trim_start_matches(':')
        .trim()
        .split_once('(')
        .and_then(|(_, arguments)| arguments.split_once(')'))
        .and_then(|(inside, _)| inside.split_once(','))
        .map(|(suite, name)| (suite.trim().to_string(), name.trim().to_string()));
    Some(PendingLine {
        file: file.to_string(),
        line,
        phase: number.parse().ok()?,
        case,
    })
}

fn is_word_char(character: char) -> bool {
    character.is_alphanumeric() || character == '_'
}

/// Where the text that follows an `ignore` word starts, for the word `ignore` inside an
/// attribute: as `#[ignore` itself, or as an argument of `#[cfg_attr(`.
fn ignore_argument_start(line: &str) -> Option<usize> {
    // Attributes sit at the start of a line, one after another, never after code.
    if !line.starts_with(ATTRIBUTE_START) {
        return None;
    }
    if let Some(at) = line.find(IGNORE_ATTRIBUTE) {
        return Some(at + IGNORE_ATTRIBUTE.len());
    }
    let from = line.find(CFG_ATTR_PREFIX)? + CFG_ATTR_PREFIX.len();
    line[from..]
        .match_indices(IGNORE_WORD)
        .map(|(at, _)| (from + at, from + at + IGNORE_WORD.len()))
        .find(|&(start, _)| {
            line[..start]
                .chars()
                .next_back()
                .is_some_and(|c| c == ',' || c == ' ')
        })
        .map(|(_, end)| end)
}

/// Finds an ignore attribute on the line. The first value is the reason (`None` when the
/// attribute has none), the second is where the attribute ends.
fn parse_ignore(line: &str) -> Option<(Option<String>, usize)> {
    let start = ignore_argument_start(line)?;
    let rest = &line[start..];
    if rest.chars().next().is_some_and(is_word_char) {
        return None;
    }
    let attribute_end = rest.find(']').map_or(line.len(), |at| start + at + 1);
    let argument = rest.trim_start();
    let reason = argument
        .strip_prefix('=')
        .and_then(|value| value.trim_start().strip_prefix('"'))
        .and_then(|quoted| quoted.split_once('"'))
        .map(|(reason, _)| reason.to_string());
    Some((reason, attribute_end))
}

/// The name of the function an attribute belongs to: on the rest of its own line, or on
/// one of the lines below.
fn function_after(rest_of_line: &str, lines: &[&str]) -> String {
    std::iter::once(rest_of_line)
        .chain(lines.iter().copied().take(FUNCTION_LOOKAHEAD))
        .find_map(|line| {
            let (_, after) = line.split_once(FUNCTION_KEYWORD)?;
            let end = after.find(|c: char| !(c.is_alphanumeric() || c == '_' || c == '$'))?;
            Some(after[..end].to_string())
        })
        .unwrap_or_else(|| "(unnamed)".to_string())
}
