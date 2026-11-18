use std::fs;

use crate::support::{empty_scratch, fixture, run_xtask};

fn generate(upstream: &std::path::Path, root: &std::path::Path, out_name: &str) -> String {
    let out = root.join(out_name);
    let outcome = run_xtask(
        root,
        &[
            "port-map",
            "--upstream-dir",
            upstream.to_str().expect("utf8 path"),
            "--root",
            root.to_str().expect("utf8 path"),
            "--out",
            out.to_str().expect("utf8 path"),
        ],
    );
    assert!(outcome.success, "port-map failed: {}", outcome.stderr);
    fs::read_to_string(out).expect("read the generated map")
}

/// The cells of the table row for `file`, without the surrounding bars.
fn row_cells(map: &str, file: &str) -> Vec<String> {
    let marker = format!("`{file}`");
    let row = map
        .lines()
        .find(|line| {
            line.split('|')
                .nth(2)
                .is_some_and(|cell| cell.trim() == marker)
        })
        .unwrap_or_else(|| panic!("no row for {file} in:\n{map}"));
    row.trim_matches('|')
        .split('|')
        .map(|cell| cell.trim().to_string())
        .collect()
}

#[test]
fn four_files_give_the_expected_rows_depths_and_line_counts() {
    // Arrange
    let root = empty_scratch("map_graph");
    let expected = fs::read_to_string(fixture("graph").join("expected.md")).expect("read expected");

    // Act
    let map = generate(&fixture("graph").join("ortools"), &root, "port-map.md");

    // Assert
    assert_eq!(map, expected);
}

#[test]
fn an_include_cycle_ends_and_puts_both_files_at_the_same_depth() {
    // Arrange
    let root = empty_scratch("map_cycle");

    // Act
    let map = generate(&fixture("cycle").join("ortools"), &root, "port-map.md");

    // Assert
    let depth = |file: &str| row_cells(&map, file)[6].clone();
    assert_eq!(depth("sat/p.h"), depth("sat/q.h"));
    assert_eq!(depth("sat/p.h"), "0");
    assert_eq!(depth("sat/r.h"), "1");
}

#[test]
fn a_header_with_a_stub_or_a_pending_line_is_partial_one_without_is_done_and_a_missing_one_is_todo()
{
    // Arrange
    let root = empty_scratch("map_status");
    fs::create_dir_all(root.join("crates")).expect("create crates");
    let rust_tree = fixture("status").join("rust/crates/x");
    let target = root.join("crates/x");
    fs::create_dir_all(target.join("src")).expect("create the module folder");
    for module in ["done.rs", "stub.rs", "pending.rs"] {
        fs::copy(
            rust_tree.join("src").join(module),
            target.join("src").join(module),
        )
        .expect("copy a module");
    }

    // Act
    let map = generate(&fixture("status").join("ortools"), &root, "port-map.md");

    // Assert
    let status = |file: &str| row_cells(&map, file)[8].clone();
    assert_eq!(status("sat/stub.h"), "partial");
    assert_eq!(status("sat/pending.h"), "partial");
    assert_eq!(status("sat/done.h"), "done");
    assert_eq!(status("sat/todo.h"), "todo");
}

#[test]
fn eight_direct_cases_and_nine_macro_uses_count_as_seventeen() {
    // Arrange
    let root = empty_scratch("map_macro_cases");

    // Act
    let map = generate(
        &fixture("upstream_macro").join("ortools"),
        &root,
        "port-map.md",
    );

    // Assert
    assert_eq!(row_cells(&map, "sat/model.h")[4], "17");
}
