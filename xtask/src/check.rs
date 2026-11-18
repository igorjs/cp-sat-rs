//! `cargo xtask port-map --check`: holds the test counts to `port-baseline.toml`.

use std::fs;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::baseline::{self, BASELINE_FILE, Baseline, FileEntry};
use crate::cargo_list::{self, ListedTest, TestLists};
use crate::rust_tree::{self, RustTree};
use crate::upstream::{self, Case, TYPED_TEST_MACRO};

pub struct CheckOptions {
    pub root: PathBuf,
    pub release: bool,
    pub upstream_dir: Option<PathBuf>,
    pub base: Option<String>,
}

/// Runs every check. `Err` means a check could not run, `Ok` holds the problems found.
pub fn run(options: &CheckOptions) -> Result<Vec<String>, String> {
    let profile = if options.release { "release" } else { "debug" };
    eprintln!("port-map: reading {BASELINE_FILE}");
    let baseline = baseline::read(&options.root)?;
    eprintln!("port-map: listing the tests of the {profile} profile");
    let lists = cargo_list::list(&options.root, options.release)?;
    eprintln!("port-map: reading the Rust tree");
    let tree = rust_tree::scan(&options.root)?;

    let mut problems = check_reasons(&baseline, &tree);
    problems.extend(check_listed(&baseline, &tree));
    eprintln!(
        "port-map: checking the counts of {} ported files",
        baseline.file.len()
    );
    for entry in &baseline.file {
        problems.extend(check_counts(entry, &lists, &tree, options.release));
    }
    if let Some(upstream_dir) = &options.upstream_dir {
        eprintln!(
            "port-map: recounting the upstream cases in {}",
            upstream_dir.display()
        );
        for entry in &baseline.file {
            problems.extend(check_upstream(
                entry,
                upstream_dir,
                &lists,
                &tree,
                options.release,
            )?);
        }
    }
    if let Some(base) = &options.base {
        eprintln!("port-map: comparing with {BASELINE_FILE} on {base}");
        problems.extend(check_base(&baseline, &options.root, base)?);
    }
    Ok(problems)
}

fn tests_of<'a>(entry: &FileEntry, tests: &'a [ListedTest]) -> Vec<&'a ListedTest> {
    tests
        .iter()
        .filter(|test| test.binary == entry.binary)
        .filter(|test| match &entry.module {
            Some(module) => test.path.starts_with(&format!("{module}::")),
            None => true,
        })
        .collect()
}

fn pending_of<'a>(entry: &FileEntry, tree: &'a RustTree) -> Vec<&'a rust_tree::PendingLine> {
    tree.pending
        .iter()
        .filter(|line| line.file == entry.rust)
        .collect()
}

/// Cases that exist in the other profile only.
fn other_profile_cases(entry: &FileEntry, release: bool) -> usize {
    if release {
        entry.debug_only
    } else {
        entry.release_only
    }
}

fn check_reasons(baseline: &Baseline, tree: &RustTree) -> Vec<String> {
    let current = baseline.phase;
    let mut problems = Vec::new();
    for test in &tree.ignored {
        let place = format!("{}:{}", test.file, test.line);
        match (&test.reason, test.phase()) {
            (None, _) => problems.push(format!("{place}: ignored test `{}` has no reason", test.name)),
            (Some(_), None) => problems.push(format!(
                "{place}: ignored test `{}` has a reason that does not start with `phase N:`",
                test.name
            )),
            (Some(_), Some(phase)) if phase <= current => problems.push(format!(
                "{place}: ignored test `{}` names phase {phase}, which is not after the current phase {current}",
                test.name
            )),
            _ => {}
        }
    }
    for line in tree.pending.iter().filter(|line| line.phase <= current) {
        problems.push(format!(
            "{}:{}: pending line names phase {}, which is not after the current phase {current}",
            line.file, line.line, line.phase
        ));
    }
    problems
}

/// A ported file that holds tests or pending lines needs an entry in the baseline.
fn check_listed(baseline: &Baseline, tree: &RustTree) -> Vec<String> {
    tree.test_holders
        .iter()
        .filter(|file| !baseline.file.iter().any(|entry| &entry.rust == *file))
        .map(|file| {
            format!("{file}: ported file has tests or pending lines but no `[[file]]` entry in {BASELINE_FILE}")
        })
        .collect()
}

fn check_counts(
    entry: &FileEntry,
    lists: &TestLists,
    tree: &RustTree,
    release: bool,
) -> Vec<String> {
    let mut problems = Vec::new();
    let found = tests_of(entry, &lists.all).len();
    let pending = pending_of(entry, tree).len();
    let other = other_profile_cases(entry, release);
    let expected = (entry.upstream_cases + entry.typed_additions + entry.added.len())
        .saturating_sub(other + pending);
    if found != expected {
        problems.push(format!(
            "{}: found {found} tests, expected {expected} ({} upstream cases, {} typed additions, \
             {} added tests, less {other} for the other profile and {pending} pending)",
            entry.rust,
            entry.upstream_cases,
            entry.typed_additions,
            entry.added.len(),
        ));
    }
    for added in entry
        .added
        .iter()
        .filter(|added| added.reason.trim().is_empty())
    {
        problems.push(format!(
            "{}: added test `{}` has no reason",
            entry.rust, added.name
        ));
    }
    let ignored = tests_of(entry, &lists.ignored).len();
    if ignored > entry.ignored {
        problems.push(format!(
            "{}: {ignored} ignored tests, the baseline allows {}",
            entry.rust, entry.ignored
        ));
    }
    if pending > entry.pending {
        problems.push(format!(
            "{}: {pending} pending lines, the baseline allows {}",
            entry.rust, entry.pending
        ));
    }
    problems
}

/// `CamelCase` to `snake_case`, keeping an acronym together: `CPModel` is `cp_model`.
pub fn snake_case(name: &str) -> String {
    let characters: Vec<char> = name.chars().collect();
    let mut out = String::new();
    for (at, &character) in characters.iter().enumerate() {
        if character.is_uppercase() && at > 0 {
            let before = characters[at - 1];
            let after_is_lower = characters.get(at + 1).is_some_and(|c| c.is_lowercase());
            if before.is_lowercase()
                || before.is_ascii_digit()
                || (before.is_uppercase() && after_is_lower)
            {
                out.push('_');
            }
        }
        out.extend(character.to_lowercase());
    }
    out
}

/// The name of the Rust test for an upstream `TEST(Suite, Name)`.
fn rust_test_name(case: &Case) -> String {
    format!("{}_{}", snake_case(&case.suite), snake_case(&case.name))
}

fn check_upstream(
    entry: &FileEntry,
    upstream_dir: &Path,
    lists: &TestLists,
    tree: &RustTree,
    release: bool,
) -> Result<Vec<String>, String> {
    let path = upstream_dir.join(&entry.upstream);
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read upstream file {}: {error}", entry.upstream))?;
    let cases = upstream::count_cases(&text);
    let mut problems = Vec::new();
    if !entry.upstream.ends_with(upstream::TEST_FILE_SUFFIX)
        && let Some(test) = upstream::test_file_for(upstream_dir, &entry.upstream)
    {
        problems.push(format!(
            "{}: upstream has the test file {test} for {}, and the entry does not name it",
            entry.rust, entry.upstream
        ));
    }
    if cases.total() != entry.upstream_cases {
        problems.push(format!(
            "{}: baseline holds {} upstream cases, upstream has {}",
            entry.rust,
            entry.upstream_cases,
            cases.total()
        ));
    }

    let tests = tests_of(entry, &lists.all);
    let pending = pending_of(entry, tree);
    let has_test = |case: &Case| {
        let wanted = rust_test_name(case);
        let exact = tests.iter().any(|test| test.name() == wanted);
        let typed = case.macro_name == TYPED_TEST_MACRO
            && tests.iter().any(|test| test.name().starts_with(&wanted));
        let pending_case = pending
            .iter()
            .any(|line| line.case.as_ref() == Some(&(case.suite.clone(), case.name.clone())));
        exact || typed || pending_case
    };
    let missing: Vec<&Case> = cases
        .written
        .iter()
        .filter(|case| !has_test(case))
        .collect();
    // A case that exists in the other profile only is missing here without being wrong.
    if missing.len() > other_profile_cases(entry, release) {
        for case in missing {
            problems.push(format!(
                "{}: upstream case `{}({}, {})` has no Rust test named `{}` and no pending line",
                entry.rust,
                case.macro_name,
                case.suite,
                case.name,
                rust_test_name(case)
            ));
        }
    }
    for added in &entry.added {
        if !tests.iter().any(|test| test.name() == added.name) {
            problems.push(format!(
                "{}: added test `{}` does not exist",
                entry.rust, added.name
            ));
        }
    }
    Ok(problems)
}

/// Reads `port-baseline.toml` as it is on `base` with `git show`.
fn base_baseline(root: &Path, base: &str) -> Result<Option<Baseline>, String> {
    let git = |args: &[&str]| {
        Command::new("git")
            .current_dir(root)
            .args(args)
            .output()
            .map_err(|error| format!("cannot run git: {error}"))
    };
    let known = git(&[
        "rev-parse",
        "--verify",
        "--quiet",
        &format!("{base}^{{commit}}"),
    ])?;
    if !known.status.success() {
        return Err(format!("the base ref `{base}` does not exist"));
    }
    let shown = git(&["show", &format!("{base}:{BASELINE_FILE}")])?;
    if !shown.status.success() {
        return Ok(None);
    }
    baseline::parse(&String::from_utf8_lossy(&shown.stdout)).map(Some)
}

fn check_base(baseline: &Baseline, root: &Path, base: &str) -> Result<Vec<String>, String> {
    let Some(before) = base_baseline(root, base)? else {
        eprintln!("port-map: {BASELINE_FILE} does not exist on {base}, nothing to compare");
        return Ok(Vec::new());
    };
    let mut problems = Vec::new();
    for old in &before.file {
        if !baseline
            .file
            .iter()
            .any(|entry| entry.upstream == old.upstream)
        {
            problems.push(format!(
                "{}: the entry exists on the base and is missing now",
                old.upstream
            ));
        }
    }
    for entry in &baseline.file {
        let Some(old) = before
            .file
            .iter()
            .find(|old| old.upstream == entry.upstream)
        else {
            continue;
        };
        if entry.ignored > old.ignored {
            problems.push(format!(
                "{}: ignored tests went from {} on the base to {}",
                entry.rust, old.ignored, entry.ignored
            ));
        }
        if entry.pending > old.pending {
            problems.push(format!(
                "{}: pending lines went from {} on the base to {}",
                entry.rust, old.pending, entry.pending
            ));
        }
    }
    Ok(problems)
}
