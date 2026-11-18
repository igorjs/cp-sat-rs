//! Compiles the C++ adapter and links the reference solver, only with the
//! feature `reference`.

#[cfg(feature = "reference")]
mod reference {
    use std::{env, fs, path::Path, path::PathBuf};

    const PREFIX_VAR: &str = "REFERENCE_PREFIX";
    const STAMP_FILE: &str = ".reference-commit";
    const BRIDGE_SOURCE: &str = "src/lib.rs";
    const ADAPTER_SOURCE: &str = "src/adapter.cc";
    const ADAPTER_HEADER: &str = "src/adapter.h";
    const CPP_STANDARD: &str = "c++20";
    // The compile definitions that the installed ortools target exports.
    const SOLVER_DEFINITIONS: [(&str, Option<&str>); 3] = [
        ("USE_BOP", None),
        ("USE_GLOP", None),
        ("OR_ORTOOLS_PROTO_DLL", Some("")),
    ];
    const SOLVER_LIBRARIES: [&str; 3] = ["ortools", "ortools_core", "protobuf"];
    const ABSEIL_FILE_PREFIX: &str = "libabsl_";
    const ABSEIL_LINK_PREFIX: &str = "absl_";
    const SHARED_EXTENSIONS: [&str; 2] = ["dylib", "so"];

    fn fail(message: &str) -> ! {
        eprintln!("error: {message}");
        std::process::exit(1);
    }

    fn reference_prefix() -> PathBuf {
        let Some(value) = env::var_os(PREFIX_VAR) else {
            fail(&format!(
                "the feature `reference` needs {PREFIX_VAR}, the install prefix of the reference \
                 solver; scripts/build-reference.sh builds it"
            ));
        };
        PathBuf::from(value)
    }

    fn read_commit(prefix: &Path) -> String {
        let stamp = prefix.join(STAMP_FILE);
        println!("cargo:rerun-if-changed={}", stamp.display());
        match fs::read_to_string(&stamp) {
            Ok(text) => text.trim().to_owned(),
            Err(error) => fail(&format!(
                "{PREFIX_VAR} is {}, which has no readable {STAMP_FILE} ({error}); it is not a \
                 finished reference build",
                prefix.display()
            )),
        }
    }

    /// Names of the unversioned abseil shared libraries, which carry the
    /// symbols the adapter and the solver headers use.
    fn abseil_libraries(lib_dir: &Path) -> Vec<String> {
        let entries = fs::read_dir(lib_dir)
            .unwrap_or_else(|error| fail(&format!("cannot list {}: {error}", lib_dir.display())));
        let mut names: Vec<String> = entries
            .filter_map(Result::ok)
            .filter_map(|entry| {
                let file_name = entry.file_name().into_string().ok()?;
                let stem = file_name.strip_prefix(ABSEIL_FILE_PREFIX)?;
                let (name, extension) = stem.rsplit_once('.')?;
                (SHARED_EXTENSIONS.contains(&extension) && !name.contains('.'))
                    .then(|| format!("{ABSEIL_LINK_PREFIX}{name}"))
            })
            .collect();
        names.sort();
        names
    }

    pub fn build() {
        let prefix = reference_prefix();
        let commit = read_commit(&prefix);
        let lib_dir = prefix.join("lib");
        let include_dir = prefix.join("include");

        println!("cargo:rerun-if-env-changed={PREFIX_VAR}");
        for source in [BRIDGE_SOURCE, ADAPTER_SOURCE, ADAPTER_HEADER] {
            println!("cargo:rerun-if-changed={source}");
        }
        println!("cargo:rustc-env=CPSAT_REFERENCE_COMMIT={commit}");

        // -isystem keeps the warnings of third-party headers out of the output
        // and, with no -I for Homebrew or the system, makes the prefix the only
        // source of abseil and protobuf headers.
        let mut build = cxx_build::bridge(BRIDGE_SOURCE);
        build
            .file(ADAPTER_SOURCE)
            .std(CPP_STANDARD)
            .flag("-isystem")
            .flag(include_dir.to_string_lossy().as_ref());
        for (name, value) in SOLVER_DEFINITIONS {
            build.define(name, value);
        }
        build.compile("cpsat_difftest_adapter");

        println!("cargo:rustc-link-search=native={}", lib_dir.display());
        for name in SOLVER_LIBRARIES
            .iter()
            .map(|name| (*name).to_owned())
            .chain(abseil_libraries(&lib_dir))
        {
            println!("cargo:rustc-link-lib=dylib={name}");
        }
        // Test and binary targets find the shared libraries through this path,
        // with no DYLD_LIBRARY_PATH or LD_LIBRARY_PATH.
        println!("cargo:rustc-link-arg=-Wl,-rpath,{}", lib_dir.display());
        // The Linux loader ignores DT_RUNPATH, the default tag, when it looks
        // for the dependencies of the shared libraries (abseil, re2). DT_RPATH
        // is searched for them too.
        if env::var("CARGO_CFG_TARGET_OS").is_ok_and(|os| os == "linux") {
            println!("cargo:rustc-link-arg=-Wl,--disable-new-dtags");
        }
    }
}

fn main() {
    #[cfg(feature = "reference")]
    reference::build();
}
