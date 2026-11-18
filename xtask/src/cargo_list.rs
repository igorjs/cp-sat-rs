//! Lists the tests of the workspace by running `cargo test -- --list`.

use std::env;
use std::ffi::OsString;
use std::io::{Read, pipe};
use std::path::Path;
use std::process::Command;

const TEST_LINE_SUFFIX: &str = ": test";
const RUNNING_PREFIX: &str = "Running ";
const DOC_TESTS_PREFIX: &str = "Doc-tests ";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ListedTest {
    /// The test binary, as in the file name of the compiled target without its hash.
    pub binary: String,
    /// The full path of the test, such as `model::tests::first`.
    pub path: String,
}

impl ListedTest {
    pub fn name(&self) -> &str {
        self.path.rsplit("::").next().unwrap_or(&self.path)
    }
}

#[derive(Debug, Default)]
pub struct TestLists {
    pub all: Vec<ListedTest>,
    pub ignored: Vec<ListedTest>,
}

pub fn list(root: &Path, release: bool) -> Result<TestLists, String> {
    Ok(TestLists {
        all: run_list(root, release, false)?,
        ignored: run_list(root, release, true)?,
    })
}

fn run_list(root: &Path, release: bool, ignored: bool) -> Result<Vec<ListedTest>, String> {
    let cargo = env::var_os("CARGO").unwrap_or_else(|| OsString::from("cargo"));
    let mut command = Command::new(cargo);
    command.current_dir(root).args(["test", "--workspace"]);
    if release {
        command.arg("--release");
    }
    command.args(["--", "--list"]);
    if ignored {
        command.arg("--ignored");
    }
    // One pipe for both streams keeps the "Running" lines of cargo in order with the
    // test names the test binaries print.
    let (mut reader, writer) = pipe().map_err(|error| format!("cannot create a pipe: {error}"))?;
    command
        .stdout(
            writer
                .try_clone()
                .map_err(|error| format!("cannot clone a pipe: {error}"))?,
        )
        .stderr(writer);
    let mut child = command
        .spawn()
        .map_err(|error| format!("cannot run cargo: {error}"))?;
    drop(command);
    let mut output = String::new();
    reader
        .read_to_string(&mut output)
        .map_err(|error| format!("cannot read the output of cargo: {error}"))?;
    let status = child
        .wait()
        .map_err(|error| format!("cannot wait for cargo: {error}"))?;
    if !status.success() {
        return Err(format!("`cargo test -- --list` failed:\n{output}"));
    }
    Ok(parse_list(&output))
}

/// Takes the binary name from a line such as
/// `Running unittests src/lib.rs (target/debug/deps/fx-0123abcd)`.
fn binary_name(running_line: &str) -> Option<String> {
    let inside = running_line.rsplit_once('(')?.1.strip_suffix(')')?;
    let file = inside.rsplit('/').next()?;
    let (name, hash) = file.rsplit_once('-')?;
    hash.chars()
        .all(|c| c.is_ascii_hexdigit())
        .then(|| name.to_string())
}

/// Reads the merged output of `cargo test -- --list`. Doc tests are left out.
pub fn parse_list(output: &str) -> Vec<ListedTest> {
    let mut binary: Option<String> = None;
    let mut tests = Vec::new();
    for line in output.lines() {
        let trimmed = line.trim();
        if trimmed.starts_with(RUNNING_PREFIX) {
            binary = binary_name(trimmed);
        } else if trimmed.starts_with(DOC_TESTS_PREFIX) {
            binary = None;
        } else if let (Some(binary), Some(path)) = (&binary, line.strip_suffix(TEST_LINE_SUFFIX)) {
            tests.push(ListedTest {
                binary: binary.clone(),
                path: path.to_string(),
            });
        }
    }
    tests
}
