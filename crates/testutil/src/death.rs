//! Ports of the death test macros `EXPECT_DEATH` and `EXPECT_DEBUG_DEATH`.

use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind},
};

/// What `unimplemented!` starts its message with.
const STUB_PREFIX: &str = "not implemented";

fn panic_message(payload: &(dyn Any + Send)) -> String {
    payload
        .downcast_ref::<String>()
        .cloned()
        .or_else(|| {
            payload
                .downcast_ref::<&str>()
                .map(|text| (*text).to_owned())
        })
        .unwrap_or_default()
}

/// Fails unless `f` panics with a message that contains `message`.
///
/// A stub, which panics with `unimplemented!`, never counts as a death.
pub fn expect_death<F: FnOnce()>(f: F, message: &str) {
    let Err(payload) = catch_unwind(AssertUnwindSafe(f)) else {
        panic!("expected a panic containing {message:?}, but the closure did not panic");
    };
    let found = panic_message(payload.as_ref());
    assert!(
        !found.starts_with(STUB_PREFIX),
        "a stub is not a death: {found:?}"
    );
    assert!(
        found.contains(message),
        "the panic message {found:?} does not contain {message:?}"
    );
}

/// Is `expect_death` in a debug build. In a release build it fails if `f` panics.
pub fn expect_debug_death<F: FnOnce()>(f: F, message: &str) {
    if cfg!(debug_assertions) {
        expect_death(f, message);
    } else if let Err(payload) = catch_unwind(AssertUnwindSafe(f)) {
        panic!(
            "the closure panicked in a release build: {:?}",
            panic_message(payload.as_ref())
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expect_death_passes_when_the_message_contains_the_text() {
        // Arrange
        let dies = || panic!("index out of range");

        // Act and Assert
        expect_death(dies, "out of range");
    }

    #[test]
    #[should_panic(expected = "did not panic")]
    fn expect_death_fails_when_the_closure_returns() {
        // Arrange
        let returns = || {};

        // Act and Assert
        expect_death(returns, "out of range");
    }

    #[test]
    #[should_panic(expected = "does not contain")]
    fn expect_death_fails_when_the_message_differs() {
        // Arrange
        let dies = || panic!("something else");

        // Act and Assert
        expect_death(dies, "out of range");
    }

    #[test]
    #[should_panic(expected = "a stub is not a death")]
    fn expect_death_fails_for_a_stub_even_with_an_empty_message() {
        // Arrange
        let stub = || unimplemented!("phase 0: x");

        // Act and Assert
        expect_death(stub, "");
    }

    #[test]
    fn expect_death_runs_each_statement_of_a_test_in_turn() {
        // Arrange
        let mut caught = 0;

        // Act
        for message in ["first", "second"] {
            expect_death(|| panic!("{message} failed"), message);
            caught += 1;
        }

        // Assert
        assert_eq!(caught, 2);
    }

    #[test]
    fn expect_debug_death_passes_when_a_debug_assertion_fails_in_debug_only() {
        // Arrange
        let closure = || {
            debug_assert!(false, "invalid interval");
            assert_eq!(1 + 1, 2);
        };

        // Act and Assert
        expect_debug_death(closure, "invalid interval");
    }

    #[test]
    #[cfg(not(debug_assertions))]
    #[should_panic(expected = "inner assertion")]
    fn expect_debug_death_fails_in_release_when_the_closure_panics() {
        // Arrange
        let closure = || assert_eq!(1 + 1, 3, "inner assertion");

        // Act and Assert
        expect_debug_death(closure, "inner assertion");
    }
}
