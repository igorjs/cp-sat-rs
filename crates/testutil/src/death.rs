//! Ports of the death test macros `EXPECT_DEATH` and `EXPECT_DEBUG_DEATH`.

use std::{
    any::Any,
    panic::{AssertUnwindSafe, catch_unwind},
};

/// What `unimplemented!` and `todo!` start their messages with.
const STUB_PREFIXES: [&str; 2] = ["not implemented", "not yet implemented"];

/// The text of a panic, or `None` when the payload is not a string, as with
/// `std::panic::panic_any(42)`.
fn panic_message(payload: &(dyn Any + Send)) -> Option<String> {
    payload.downcast_ref::<String>().cloned().or_else(|| {
        payload
            .downcast_ref::<&str>()
            .map(|text| (*text).to_owned())
    })
}

/// Fails unless `f` panics with a message that contains `message`.
///
/// A stub, which panics with `unimplemented!` or `todo!`, never counts as a death, and
/// neither does a panic whose payload is not a string.
pub fn expect_death<F: FnOnce()>(f: F, message: &str) {
    let Err(payload) = catch_unwind(AssertUnwindSafe(f)) else {
        panic!("expected a panic containing {message:?}, but the closure did not panic");
    };
    let Some(found) = panic_message(payload.as_ref()) else {
        panic!("the panic payload is not a string, so it is not a death");
    };
    assert!(
        !STUB_PREFIXES.iter().any(|prefix| found.starts_with(prefix)),
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
            panic_message(payload.as_ref()).unwrap_or_default()
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
    #[should_panic(expected = "a stub is not a death")]
    fn expect_death_fails_for_a_todo_even_with_an_empty_message() {
        // Arrange
        let stub = || todo!("later");

        // Act and Assert
        expect_death(stub, "");
    }

    #[test]
    #[should_panic(expected = "payload is not a string")]
    fn expect_death_fails_for_a_panic_whose_payload_is_not_a_string() {
        // Arrange
        let dies = || std::panic::panic_any(42_u32);

        // Act and Assert
        expect_death(dies, "");
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
