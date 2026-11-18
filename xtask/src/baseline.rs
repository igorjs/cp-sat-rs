//! `port-baseline.toml`: the current phase and the test counts of each ported test file.

use std::fs;
use std::path::Path;

use serde::Deserialize;

pub const BASELINE_FILE: &str = "port-baseline.toml";

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Baseline {
    pub phase: u32,
    #[serde(default)]
    pub file: Vec<FileEntry>,
}

/// A test the port adds to a ported file, with the reason it exists.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct AddedTest {
    pub name: String,
    pub reason: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct FileEntry {
    /// The upstream test file, relative to `ortools/`.
    pub upstream: String,
    /// The Rust file that holds the port, relative to the repo root.
    pub rust: String,
    /// The test binary the Rust tests run in, as `cargo test` names it.
    pub binary: String,
    /// Restricts the binary to the tests under this module path.
    pub module: Option<String>,
    pub upstream_cases: usize,
    /// The Rust tests that each typed test adds over its one upstream case.
    #[serde(default)]
    pub typed_additions: usize,
    #[serde(default)]
    pub added: Vec<AddedTest>,
    /// Upstream cases that exist in a debug build only.
    #[serde(default)]
    pub debug_only: usize,
    /// Upstream cases that exist in a release build only.
    #[serde(default)]
    pub release_only: usize,
    #[serde(default)]
    pub pending: usize,
    #[serde(default)]
    pub ignored: usize,
}

pub fn parse(text: &str) -> Result<Baseline, String> {
    toml::from_str(text).map_err(|error| format!("invalid {BASELINE_FILE}: {error}"))
}

pub fn read(root: &Path) -> Result<Baseline, String> {
    let path = root.join(BASELINE_FILE);
    let text = fs::read_to_string(&path)
        .map_err(|error| format!("cannot read {}: {error}", path.display()))?;
    parse(&text)
}
