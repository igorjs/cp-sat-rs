use cpsat_proto::CpModelProto;
use cpsat_testutil::{fixture, parse_test_proto};
use prost::Message;

const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/parsed.txt");
const SNIPPET_COUNT: usize = 10;

#[test]
fn every_snippet_decodes_to_the_message_of_the_reference_parser() {
    // Arrange
    let cases = fixture::read(FIXTURE);

    for case in &cases {
        // Act
        let parsed: CpModelProto = parse_test_proto(case.single("text"));

        // Assert
        let expected = CpModelProto::decode(fixture::from_hex(case.single("bytes")).as_slice())
            .expect("the captured bytes decode");
        assert_eq!(parsed, expected, "{}", case.single("text"));
    }
    assert_eq!(cases.len(), SNIPPET_COUNT);
}
