//! Port of `ParseTestProto` from `base/parse_test_proto.h`.

use std::sync::OnceLock;

use cpsat_proto::FILE_DESCRIPTOR_SET;
use prost::Message;
use prost_reflect::{DescriptorPool, DynamicMessage};

fn pool() -> &'static DescriptorPool {
    static POOL: OnceLock<DescriptorPool> = OnceLock::new();
    POOL.get_or_init(|| {
        DescriptorPool::decode(FILE_DESCRIPTOR_SET)
            .expect("the descriptor set of cpsat-proto decodes")
    })
}

/// Parses protobuf text format into the message `M`, and panics on bad text.
pub fn parse_test_proto<M: Message + prost::Name + Default>(text: &str) -> M {
    let name = M::full_name();
    let descriptor = pool()
        .get_message_by_name(&name)
        .unwrap_or_else(|| panic!("no message {name} in the descriptor set"));
    DynamicMessage::parse_text_format(descriptor, text)
        .unwrap_or_else(|error| panic!("cannot parse the text of {name}: {error}"))
        .transcode_to::<M>()
        .unwrap_or_else(|error| panic!("cannot convert the text of {name}: {error}"))
}

#[cfg(test)]
mod tests {
    use cpsat_proto::{CpModelProto, SatParameters};

    use super::*;

    #[test]
    fn parses_a_model_with_a_negative_number_and_a_comment() {
        // Arrange
        let text = "# a comment line\nvariables { domain: [ -5, 0x10 ] }\n";

        // Act
        let model: CpModelProto = parse_test_proto(text);

        // Assert
        assert_eq!(model.variables[0].domain, vec![-5, 16]);
    }

    #[test]
    fn reads_a_hexadecimal_and_an_octal_number() {
        // Arrange
        let text = "random_seed: 0x2A max_memory_in_mb: 010";

        // Act
        let parameters: SatParameters = parse_test_proto(text);

        // Assert
        assert_eq!(parameters.random_seed(), 42);
        assert_eq!(parameters.max_memory_in_mb(), 8);
    }

    #[test]
    fn panics_with_the_name_of_an_unknown_field() {
        // Arrange
        let text = "variables { not_a_field: 1 }";

        // Act
        let message = std::panic::catch_unwind(|| parse_test_proto::<CpModelProto>(text))
            .expect_err("unknown field must panic");

        // Assert
        let message = message.downcast_ref::<String>().expect("a string message");
        assert!(message.contains("not_a_field"), "{message}");
    }
}
