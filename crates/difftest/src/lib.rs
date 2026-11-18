//! Calls into the C++ reference solver, to compare the Rust port against it.
//!
//! The crate is empty unless the feature `reference` is on. Every call returns
//! `Err` for a C++ exception or a parse failure, and never aborts.

#[cfg(feature = "reference")]
#[cxx::bridge(namespace = "cpsat_difftest")]
mod ffi {
    unsafe extern "C++" {
        include!("cpsat-difftest/src/adapter.h");

        /// Solves a `CpModelProto` under `SatParameters`, both as bytes, and
        /// returns the `CpSolverResponse` bytes.
        fn reference_solve(model_bytes: &[u8], parameter_bytes: &[u8]) -> Result<Vec<u8>>;

        /// Parses protobuf text into the named message and returns its bytes.
        fn reference_parse_text(message_name: &str, text: &str) -> Result<Vec<u8>>;

        /// Prints the message in `bytes` as `ProtobufShortDebugString` does, or
        /// as `ProtobufDebugString` does under the two limits.
        fn reference_debug_string(
            message_name: &str,
            bytes: &[u8],
            short_form: bool,
            max_chars: i32,
            max_lines: i32,
        ) -> Result<String>;

        /// Returns the error text of `ValidateCpModel`, empty when the model is valid.
        fn reference_validate(model_bytes: &[u8]) -> Result<String>;

        /// Returns the error text of `ValidateInputCpModel`, empty when the input is valid.
        fn reference_validate_input(parameter_bytes: &[u8], model_bytes: &[u8]) -> Result<String>;

        /// Returns what `SolutionIsFeasible` says of `values` for the model.
        fn reference_solution_is_feasible(model_bytes: &[u8], values: &[i64]) -> Result<bool>;

        /// Returns the string of `CpSatSolverVersion`.
        fn reference_version() -> Result<String>;

        /// Returns `fasthash64` of the bytes under the seed.
        fn reference_fasthash64(bytes: &[u8], seed: u64) -> Result<u64>;
    }
}

#[cfg(feature = "reference")]
pub use ffi::{
    reference_debug_string, reference_fasthash64, reference_parse_text,
    reference_solution_is_feasible, reference_solve, reference_validate, reference_validate_input,
    reference_version,
};

/// The commit of OR-Tools the linked reference solver was built from.
#[cfg(feature = "reference")]
pub const REFERENCE_COMMIT: &str = env!("CPSAT_REFERENCE_COMMIT");

#[cfg(all(test, feature = "reference"))]
mod tests {
    use cpsat_proto::{
        CpModelProto, CpObjectiveProto, CpSolverResponse, CpSolverStatus, IntegerVariableProto,
        SatParameters,
    };
    use prost::Message;

    use super::*;

    const MODEL_NAME: &str = "operations_research.sat.CpModelProto";
    const NOT_A_MESSAGE: [u8; 4] = [0xFF; 4];
    const MAX_VALUE: i64 = 10;

    fn variable(domain: &[i64]) -> IntegerVariableProto {
        IntegerVariableProto {
            domain: domain.to_vec(),
            ..Default::default()
        }
    }

    fn maximise_x_model() -> CpModelProto {
        CpModelProto {
            variables: vec![variable(&[0, MAX_VALUE])],
            objective: Some(CpObjectiveProto {
                vars: vec![0],
                coeffs: vec![-1],
                scaling_factor: -1.0,
                ..Default::default()
            }),
            ..Default::default()
        }
    }

    #[test]
    fn solves_a_one_variable_model() {
        // Arrange
        let model = maximise_x_model().encode_to_vec();
        let parameters = SatParameters::default().encode_to_vec();

        // Act
        let response_bytes = reference_solve(&model, &parameters).unwrap();

        // Assert
        let response = CpSolverResponse::decode(response_bytes.as_slice()).unwrap();
        assert_eq!(response.status(), CpSolverStatus::Optimal);
        assert_eq!(response.solution, vec![MAX_VALUE]);
    }

    #[test]
    fn returns_an_error_for_bytes_that_are_not_a_model() {
        // Arrange
        let parameters = SatParameters::default().encode_to_vec();

        // Act
        let result = reference_solve(&NOT_A_MESSAGE, &parameters);

        // Assert
        let error = result.unwrap_err();
        assert!(error.what().contains("CpModelProto"), "{error}");
    }

    #[test]
    fn parses_text_into_message_bytes() {
        // Arrange
        let text = "variables { domain: 0 domain: 1 }";

        // Act
        let bytes = reference_parse_text(MODEL_NAME, text).unwrap();

        // Assert
        let model = CpModelProto::decode(bytes.as_slice()).unwrap();
        assert_eq!(model.variables.len(), 1);
        assert_eq!(model.variables[0].domain, vec![0, 1]);
    }

    #[test]
    fn validate_reports_an_empty_domain() {
        // Arrange
        let model = CpModelProto {
            variables: vec![variable(&[])],
            ..Default::default()
        }
        .encode_to_vec();

        // Act
        let text = reference_validate(&model).unwrap();

        // Assert
        assert!(!text.is_empty());
    }

    #[test]
    fn validate_accepts_a_valid_model() {
        // Arrange
        let model = maximise_x_model().encode_to_vec();
        let parameters = SatParameters::default().encode_to_vec();

        // Act
        let model_text = reference_validate(&model).unwrap();
        let input_text = reference_validate_input(&parameters, &model).unwrap();

        // Assert
        assert_eq!(model_text, "");
        assert_eq!(input_text, "");
    }

    #[test]
    fn solution_is_feasible_checks_the_domain() {
        // Arrange
        let model = maximise_x_model().encode_to_vec();

        // Act
        let inside = reference_solution_is_feasible(&model, &[MAX_VALUE]).unwrap();
        let outside = reference_solution_is_feasible(&model, &[MAX_VALUE + 1]).unwrap();

        // Assert
        assert!(inside);
        assert!(!outside);
    }

    #[test]
    fn debug_string_prints_short_and_long_forms() {
        // Arrange
        let bytes = reference_parse_text(MODEL_NAME, "variables { domain: 0 domain: 1 }").unwrap();

        // Act
        let short = reference_debug_string(MODEL_NAME, &bytes, true, 0, 0).unwrap();
        let long = reference_debug_string(MODEL_NAME, &bytes, false, i32::MAX, i32::MAX).unwrap();

        // Assert
        assert_eq!(short, "variables { domain: 0 domain: 1 }");
        assert!(
            long.starts_with("# [operations_research.sat.CpModelProto of "),
            "{long}"
        );
    }

    #[test]
    fn version_names_a_release() {
        // Act
        let version = reference_version().unwrap();

        // Assert
        assert!(!version.is_empty());
    }

    #[test]
    fn fasthash64_depends_on_the_seed() {
        // Arrange
        let bytes = b"cp-sat";

        // Act
        let first = reference_fasthash64(bytes, 1).unwrap();
        let again = reference_fasthash64(bytes, 1).unwrap();
        let other_seed = reference_fasthash64(bytes, 2).unwrap();

        // Assert
        assert_eq!(first, again);
        assert_ne!(first, other_seed);
    }
}
