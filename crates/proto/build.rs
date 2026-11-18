//! Compiles the committed proto files in Rust, so the build needs no `protoc`.

use std::{env, error::Error, fs, path::PathBuf};

use protox::prost::Message;

const PROTO_ROOT: &str = "proto";
const PROTO_FILES: [&str; 2] = [
    "ortools/sat/cp_model.proto",
    "ortools/sat/sat_parameters.proto",
];
const DESCRIPTOR_FILE_NAME: &str = "file_descriptor_set.bin";

fn main() -> Result<(), Box<dyn Error>> {
    println!("cargo:rerun-if-changed={PROTO_ROOT}");

    let descriptors = protox::compile(PROTO_FILES, [PROTO_ROOT])?;
    let out_dir = PathBuf::from(env::var("OUT_DIR")?);
    fs::write(
        out_dir.join(DESCRIPTOR_FILE_NAME),
        descriptors.encode_to_vec(),
    )?;
    // The type names let a caller find the descriptor of a generated type.
    prost_build::Config::new()
        .enable_type_names()
        .compile_fds(descriptors)?;
    Ok(())
}
