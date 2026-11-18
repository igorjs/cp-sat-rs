//! Builds `docs/port-map.md`: one row per upstream file, with its place in the
//! dependency order and its status in the Rust tree.

use std::collections::BTreeMap;
use std::fs;
use std::path::Path;

use crate::graph;
use crate::phases::{PHASE_COUNT, phase_of, phased_files};
use crate::render::{self, Map, PhaseCounts, Row, Totals};
use crate::rust_tree::{self, RustTree};
use crate::upstream;

const SOURCE_SUFFIX: &str = ".cc";

pub fn generate(upstream_dir: &Path, root: &Path, out: &Path) -> Result<(), String> {
    eprintln!(
        "port-map: reading upstream source in {}",
        upstream_dir.display()
    );
    let (sat_sources, sat_tests) = upstream::sat_files(upstream_dir)?;
    let closure = upstream::include_closure(upstream_dir, &sat_sources)?;
    // Test helpers are not in `sat_sources`, yet the port set holds some, so a file in the
    // phase table starts a closure of its own.
    let mut mapped_start = sat_sources.clone();
    mapped_start.extend(
        phased_files()
            .filter(|path| upstream_dir.join(path).is_file())
            .map(String::from),
    );
    let mapped = upstream::include_closure(upstream_dir, &mapped_start)?;

    eprintln!("port-map: reading the Rust tree in {}", root.display());
    let tree = rust_tree::scan(root)?;

    eprintln!("port-map: counting test cases and depths");
    let includes: BTreeMap<String, Vec<String>> = mapped
        .iter()
        .map(|(path, file)| (path.clone(), file.includes.iter().cloned().collect()))
        .collect();
    let depths = graph::depths(&includes);

    let mut rows = Vec::new();
    for (path, file) in &mapped {
        let test = upstream::test_file_for(upstream_dir, path);
        let cases = match &test {
            Some(test) => Some(count_test_cases(upstream_dir, test)?),
            None => None,
        };
        rows.push(Row {
            folder: path.split('/').next().unwrap_or_default().to_string(),
            path: path.clone(),
            lines: file.lines,
            test,
            cases,
            includes: includes[path].clone(),
            depth: depths[path],
            phase: phase_of(path),
            status: status_of(path, &tree),
        });
    }

    let totals = totals(upstream_dir, &sat_sources, &sat_tests, &closure)?;
    let map = Map {
        totals,
        phases: phase_counts(&tree),
        rows,
    };
    write_map(out, &render::render(&map))
}

fn count_test_cases(upstream_dir: &Path, test: &str) -> Result<usize, String> {
    let text = fs::read_to_string(upstream_dir.join(test))
        .map_err(|error| format!("cannot read upstream file {test}: {error}"))?;
    Ok(upstream::count_cases(&text).total())
}

fn status_of(path: &str, tree: &RustTree) -> &'static str {
    let stem = path.rsplit_once('.').map_or(path, |(stem, _)| stem);
    match tree.ported.get(stem) {
        None => "todo",
        Some(true) => "partial",
        Some(false) => "done",
    }
}

fn totals(
    upstream_dir: &Path,
    sat_sources: &[String],
    sat_tests: &[String],
    closure: &BTreeMap<String, upstream::SourceFile>,
) -> Result<Totals, String> {
    let mut test_lines = 0;
    let mut test_cases = 0;
    for test in sat_tests {
        let text = fs::read_to_string(upstream_dir.join(test))
            .map_err(|error| format!("cannot read upstream file {test}: {error}"))?;
        test_lines += upstream::line_count(&text);
        test_cases += upstream::count_cases(&text).total();
    }
    let sat_source_lines = sat_sources.iter().map(|path| closure[path].lines).sum();
    let sat_cc: Vec<&String> = sat_sources
        .iter()
        .filter(|path| path.ends_with(SOURCE_SUFFIX))
        .collect();
    let with_test = sat_cc
        .iter()
        .filter(|path| upstream::test_file_for(upstream_dir, path).is_some())
        .count();
    Ok(Totals {
        sat_files: sat_sources.len(),
        sat_lines: sat_source_lines,
        test_files: sat_tests.len(),
        test_lines,
        test_cases,
        closure_files: closure.len(),
        closure_lines: closure.values().map(|file| file.lines).sum(),
        sat_cc_files: sat_cc.len(),
        sat_cc_with_test: with_test,
    })
}

fn phase_counts(tree: &RustTree) -> Vec<PhaseCounts> {
    (0..PHASE_COUNT)
        .map(|phase| PhaseCounts {
            phase,
            ignored: tree
                .ignored
                .iter()
                .filter(|test| test.phase() == Some(phase))
                .count(),
            pending: tree
                .pending
                .iter()
                .filter(|line| line.phase == phase)
                .count(),
        })
        .collect()
}

fn write_map(out: &Path, text: &str) -> Result<(), String> {
    if let Some(parent) = out.parent() {
        fs::create_dir_all(parent)
            .map_err(|error| format!("cannot create {}: {error}", parent.display()))?;
    }
    fs::write(out, text).map_err(|error| format!("cannot write {}: {error}", out.display()))?;
    eprintln!("port-map: wrote {}", out.display());
    Ok(())
}
