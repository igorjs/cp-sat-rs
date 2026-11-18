use std::fs;

use crate::support::{fixture, run_git, run_xtask, scratch_copy};

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
