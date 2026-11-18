//! Wire types of the CP-SAT model and the solver parameters, generated from the
//! committed proto files.

#[allow(
    clippy::all,
    clippy::pedantic,
    clippy::nursery,
    missing_docs,
    unreachable_pub
)]
mod generated {
    include!(concat!(env!("OUT_DIR"), "/operations_research.sat.rs"));
}

pub use generated::*;

/// The encoded `FileDescriptorSet` of the two proto files, for runtime reflection.
pub const FILE_DESCRIPTOR_SET: &[u8] =
    include_bytes!(concat!(env!("OUT_DIR"), "/file_descriptor_set.bin"));

#[cfg(test)]
mod tests {
    use std::{fs, path::Path};

    use prost::Message;
    use sha2::{Digest, Sha256};

    use super::*;

    const PROTO_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/proto");
    const SAT_DIR: &str = "ortools/sat";

    #[test]
    fn cp_model_round_trips() {
        // Arrange
        let model = CpModelProto {
            name: "round trip".to_owned(),
            variables: vec![IntegerVariableProto {
                name: "x".to_owned(),
                domain: vec![0, 10],
            }],
            constraints: vec![ConstraintProto {
                constraint: Some(constraint_proto::Constraint::Linear(
                    LinearConstraintProto {
                        vars: vec![0],
                        coeffs: vec![2],
                        domain: vec![0, 6],
                    },
                )),
                ..Default::default()
            }],
            ..Default::default()
        };

        // Act
        let decoded = CpModelProto::decode(model.encode_to_vec().as_slice())
            .expect("the encoded model decodes");

        // Assert
        assert_eq!(decoded, model);
    }

    #[test]
    fn parameter_defaults_match_the_proto_file() {
        // Arrange
        let parameters = SatParameters::default();

        // Act and Assert
        // bool, sat_parameters.proto line 982
        assert!(parameters.use_precedences_in_disjunctive_constraint());
        // int32, sat_parameters.proto line 401
        assert_eq!(parameters.random_seed(), 1);
        // int64, sat_parameters.proto line 367
        assert_eq!(parameters.max_memory_in_mb(), 10000);
        // double, sat_parameters.proto line 250
        assert_eq!(parameters.variable_activity_decay(), 0.8);
        // enum, sat_parameters.proto line 59
        assert_eq!(
            parameters.initial_polarity(),
            sat_parameters::Polarity::False
        );
    }

    #[test]
    fn proto_files_match_the_recorded_hashes() {
        // Arrange
        let readme = fs::read_to_string(Path::new(PROTO_DIR).join("README.md"))
            .expect("README.md is readable");

        for file in ["cp_model.proto", "sat_parameters.proto"] {
            // Act
            let bytes = fs::read(Path::new(PROTO_DIR).join(SAT_DIR).join(file))
                .expect("the proto file is readable");
            let hash = format!("{:x}", Sha256::digest(bytes));

            // Assert
            let line = readme
                .lines()
                .find(|line| line.contains(file))
                .unwrap_or_else(|| panic!("README.md has no line for {file}"));
            assert!(line.contains(&hash), "{file} hash differs from README.md");
        }
    }
}
