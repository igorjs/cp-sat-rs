use crate::support::{
    ALWAYS_COLOUR, empty_scratch, run_check_red, run_check_red_with_env, scratch_copy,
    scratch_copy_as, write_coloured_fake_cargo,
};

#[test]
fn an_ignored_test_that_fails_an_assertion_fails_the_script_and_is_named() {
    // Arrange
    let root = scratch_copy("red_mixed");

    // Act
    let outcome = run_check_red(&root);

    // Assert
    assert!(!outcome.success);
    assert!(
        outcome.all_output().contains("wrong_reason"),
        "{}",
        outcome.all_output()
    );
    assert!(!outcome.all_output().contains("not red: tests::stubbed"));
}

#[test]
fn an_ignored_test_that_only_hits_a_stub_passes_the_script() {
    // Arrange
    let root = scratch_copy("red_ok");

    // Act
    let outcome = run_check_red(&root);

    // Assert
    assert!(outcome.success, "{}", outcome.all_output());
}

#[test]
fn no_ignored_test_passes_the_script() {
    // Arrange
    let root = scratch_copy("red_none");

    // Act
    let outcome = run_check_red(&root);

    // Assert
    assert!(outcome.success, "{}", outcome.all_output());
}

#[test]
fn a_failing_ignored_test_in_the_first_binary_does_not_stop_the_second() {
    // Arrange
    let root = scratch_copy("red_two_binaries");

    // Act
    let outcome = run_check_red(&root);

    // Assert
    assert!(outcome.success, "{}", outcome.all_output());
    assert!(
        outcome.stdout.contains("2 ignored tests"),
        "{}",
        outcome.stdout
    );
}

#[test]
fn an_ignored_test_that_passes_fails_the_script() {
    // Arrange
    let root = scratch_copy("red_passing");

    // Act
    let outcome = run_check_red(&root);

    // Assert
    assert!(!outcome.success);
    assert!(
        outcome.all_output().contains("quietly_green"),
        "{}",
        outcome.all_output()
    );
}

#[test]
fn coloured_cargo_output_does_not_hide_the_ignored_tests() {
    // Arrange
    let root = scratch_copy_as("red_two_binaries", "red_colour");

    // Act
    let outcome = run_check_red_with_env(&root, &[("CARGO_TERM_COLOR", ALWAYS_COLOUR)]);

    // Assert
    assert!(outcome.success, "{}", outcome.all_output());
    assert!(
        outcome.stdout.contains("2 ignored tests"),
        "{}",
        outcome.stdout
    );
}

#[test]
fn test_lines_that_follow_no_recognised_binary_fail_the_script() {
    // Arrange
    let root = scratch_copy_as("red_none", "red_unrecognised");
    let bin = empty_scratch("fake_cargo_red");
    write_coloured_fake_cargo(&bin);
    let path = format!(
        "{}:{}",
        bin.display(),
        std::env::var("PATH").expect("PATH is set")
    );

    // Act
    let outcome = run_check_red_with_env(&root, &[("PATH", &path)]);

    // Assert
    assert!(!outcome.success, "{}", outcome.all_output());
    assert!(
        outcome.stderr.contains("no test binary"),
        "{}",
        outcome.stderr
    );
}
