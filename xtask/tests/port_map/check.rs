use std::fs;

use crate::support::{
    ALWAYS_COLOUR, empty_scratch, fixture, run_git, run_xtask, run_xtask_with_env, scratch_copy,
    scratch_copy_as, write_coloured_fake_cargo,
};

const CHECK: [&str; 2] = ["port-map", "--check"];

fn check_fixture(name: &str, extra: &[&str]) -> crate::support::Outcome {
    let root = scratch_copy(name);
    let mut args = CHECK.to_vec();
    args.extend_from_slice(extra);
    run_xtask(&root, &args)
}

#[test]
fn two_ignored_tests_against_a_baseline_of_one_fail_and_name_the_count() {
    // Arrange, Act
    let outcome = check_fixture("ignored_over", &[]);

    // Assert
    assert!(!outcome.success);
    assert!(outcome.stderr.contains("src/lib.rs"), "{}", outcome.stderr);
    assert!(
        outcome.stderr.contains("2 ignored tests"),
        "{}",
        outcome.stderr
    );
    assert!(outcome.stderr.contains("allows 1"), "{}", outcome.stderr);
}

#[test]
fn an_ignore_that_names_the_current_phase_fails_and_names_the_test() {
    // Arrange, Act
    let outcome = check_fixture("ignore_phase_zero", &[]);

    // Assert
    assert!(!outcome.success);
    assert!(outcome.stderr.contains("blocked"), "{}", outcome.stderr);
    assert!(outcome.stderr.contains("phase 0"), "{}", outcome.stderr);
}

#[test]
fn three_tests_and_one_pending_line_against_a_baseline_of_four_fail_and_name_the_file() {
    // Arrange, Act
    let outcome = check_fixture("pending_mismatch", &[]);

    // Assert
    assert!(!outcome.success);
    assert!(outcome.stderr.contains("src/lib.rs"), "{}", outcome.stderr);
    assert!(outcome.stderr.contains("3 tests"), "{}", outcome.stderr);
    assert!(outcome.stderr.contains("expected 4"), "{}", outcome.stderr);
}

#[test]
fn a_macro_that_makes_two_tests_counts_both() {
    // Arrange, Act
    let outcome = check_fixture("macro_tests", &[]);

    // Assert
    assert!(outcome.success, "{}", outcome.stderr);
}

#[test]
fn a_baseline_of_five_upstream_cases_against_six_fails_with_upstream() {
    // Arrange
    let upstream = scratch_copy("upstream_count").join("ortools");
    let upstream = upstream.to_str().expect("utf8 path").to_string();

    // Act
    let outcome = check_fixture(
        "upstream_count",
        &["--upstream", "--upstream-dir", &upstream],
    );

    // Assert
    assert!(!outcome.success);
    assert!(
        outcome
            .stderr
            .contains("baseline holds 5 upstream cases, upstream has 6"),
        "{}",
        outcome.stderr
    );
}

#[test]
fn an_ignore_with_no_reason_fails_and_names_the_test() {
    // Arrange, Act
    let outcome = check_fixture("ignore_no_reason", &[]);

    // Assert
    assert!(!outcome.success);
    assert!(outcome.stderr.contains("lazy"), "{}", outcome.stderr);
    assert!(outcome.stderr.contains("no reason"), "{}", outcome.stderr);
}

#[test]
fn an_ignored_count_that_grew_since_the_base_fails() {
    // Arrange
    let root = scratch_copy("base_growth");
    run_git(&root, &["init", "--quiet", "--initial-branch=main"]);
    fs::copy(
        root.join("base-baseline.toml"),
        root.join("port-baseline.toml"),
    )
    .expect("write the base baseline");
    run_git(&root, &["add", "port-baseline.toml"]);
    run_git(&root, &["commit", "--quiet", "--message", "base"]);
    run_git(&root, &["branch", "base"]);
    fs::copy(
        fixture("base_growth").join("port-baseline.toml"),
        root.join("port-baseline.toml"),
    )
    .expect("write the head baseline");

    // Act
    let outcome = run_xtask(&root, &["port-map", "--check", "--base", "base"]);

    // Assert
    assert!(!outcome.success);
    assert!(
        outcome
            .stderr
            .contains("ignored tests went from 2 on the base to 3"),
        "{}",
        outcome.stderr
    );
}

#[test]
fn an_ignore_inside_the_fixtures_folder_is_not_counted() {
    // Arrange, Act
    let outcome = check_fixture("exclusion", &[]);

    // Assert
    assert!(outcome.success, "{}", outcome.stderr);
}

#[test]
fn coloured_cargo_output_does_not_hide_the_tests() {
    // Arrange
    let root = scratch_copy_as("macro_tests", "check_colour");

    // Act
    let outcome = run_xtask_with_env(&root, &CHECK, &[("CARGO_TERM_COLOR", ALWAYS_COLOUR)]);

    // Assert
    assert!(outcome.success, "{}", outcome.stderr);
}

#[test]
fn test_lines_that_follow_no_recognised_binary_fail_the_check() {
    // Arrange
    let root = scratch_copy_as("macro_tests", "check_unrecognised");
    let bin = empty_scratch("fake_cargo_check");
    let cargo = write_coloured_fake_cargo(&bin);

    // Act
    let outcome = run_xtask_with_env(&root, &CHECK, &[("CARGO", cargo.to_str().expect("utf8"))]);

    // Assert
    assert!(!outcome.success);
    assert!(
        outcome.stderr.contains("no test binary"),
        "{}",
        outcome.stderr
    );
}

#[test]
fn a_baseline_pending_number_above_the_real_pending_lines_fails_the_count() {
    // Arrange, Act
    let outcome = check_fixture("pending_overstated", &[]);

    // Assert
    assert!(!outcome.success);
    assert!(outcome.stderr.contains("2 tests"), "{}", outcome.stderr);
    assert!(outcome.stderr.contains("expected 3"), "{}", outcome.stderr);
}

#[test]
fn a_ported_file_with_tests_and_no_baseline_entry_fails_and_names_the_file() {
    // Arrange, Act
    let outcome = check_fixture("unlisted_port", &[]);

    // Assert
    assert!(!outcome.success);
    assert!(outcome.stderr.contains("src/lib.rs"), "{}", outcome.stderr);
    assert!(
        outcome.stderr.contains("no `[[file]]` entry"),
        "{}",
        outcome.stderr
    );
}

#[test]
fn an_entry_that_exists_on_the_base_and_is_missing_now_fails() {
    // Arrange
    let root = scratch_copy_as("base_growth", "base_removed");
    run_git(&root, &["init", "--quiet", "--initial-branch=main"]);
    fs::copy(
        root.join("base-baseline.toml"),
        root.join("port-baseline.toml"),
    )
    .expect("write the base baseline");
    run_git(&root, &["add", "port-baseline.toml"]);
    run_git(&root, &["commit", "--quiet", "--message", "base"]);
    run_git(&root, &["branch", "base"]);
    fs::write(root.join("port-baseline.toml"), "phase = 0\n").expect("write the head baseline");

    // Act
    let outcome = run_xtask(&root, &["port-map", "--check", "--base", "base"]);

    // Assert
    assert!(!outcome.success);
    assert!(
        outcome
            .stderr
            .contains("sat/model_test.cc: the entry exists on the base and is missing now"),
        "{}",
        outcome.stderr
    );
}

#[test]
fn a_test_header_split_over_two_lines_is_matched_by_name() {
    // Arrange
    let upstream = scratch_copy("upstream_split").join("ortools");
    let upstream = upstream.to_str().expect("utf8 path").to_string();

    // Act
    let outcome = check_fixture(
        "upstream_split",
        &["--upstream", "--upstream-dir", &upstream],
    );

    // Assert
    assert!(outcome.success, "{}", outcome.stderr);
}

#[test]
fn an_ignore_on_the_same_line_as_the_test_attribute_is_read() {
    // Arrange, Act
    let outcome = check_fixture("ignore_one_line", &[]);

    // Assert
    assert!(!outcome.success);
    assert!(outcome.stderr.contains("`blocked`"), "{}", outcome.stderr);
    assert!(outcome.stderr.contains("phase 0"), "{}", outcome.stderr);
}

#[test]
fn an_ignore_inside_cfg_attr_is_read() {
    // Arrange, Act
    let outcome = check_fixture("ignore_cfg_attr", &[]);

    // Assert
    assert!(!outcome.success);
    assert!(outcome.stderr.contains("`blocked`"), "{}", outcome.stderr);
    assert!(outcome.stderr.contains("phase 0"), "{}", outcome.stderr);
}

fn macro_names_args(root: &std::path::Path) -> [String; 3] {
    let upstream = root.join("ortools");
    [
        "--upstream".to_string(),
        "--upstream-dir".to_string(),
        upstream.to_str().expect("utf8 path").to_string(),
    ]
}

#[test]
fn each_use_of_a_wrapper_macro_matches_a_rust_test_or_a_pending_line() {
    // Arrange
    let root = scratch_copy("macro_names");
    let args = macro_names_args(&root);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    // Act
    let outcome = run_xtask(&root, &[&CHECK[..], &args[..]].concat());

    // Assert
    assert!(outcome.success, "{}", outcome.stderr);
}

#[test]
fn a_use_of_a_wrapper_macro_with_no_rust_test_fails_and_names_the_test() {
    // Arrange
    let root = scratch_copy_as("macro_names", "macro_names_missing");
    let lib = root.join("src/lib.rs");
    let text = fs::read_to_string(&lib).expect("read the Rust file");
    fs::write(&lib, text.replace("cap_sub_generic", "cap_subtract")).expect("rename a test");
    let args = macro_names_args(&root);
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    // Act
    let outcome = run_xtask(&root, &[&CHECK[..], &args[..]].concat());

    // Assert
    assert!(!outcome.success);
    assert!(
        outcome
            .stderr
            .contains("saturated_arithmetic_test_cap_sub_generic"),
        "{}",
        outcome.stderr
    );
}
