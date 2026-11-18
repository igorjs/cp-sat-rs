use std::fs;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

const CARGO_JOBS: &str = "4";

/// Variables of the outer test run that would change how a fixture crate builds.
const INHERITED_BUILD_VARS: [&str; 5] = [
    "RUSTFLAGS",
    "CARGO_ENCODED_RUSTFLAGS",
    "RUSTDOCFLAGS",
    "CARGO_TARGET_DIR",
    "LLVM_PROFILE_FILE",
];

pub struct Outcome {
    pub success: bool,
    pub stdout: String,
    pub stderr: String,
}

impl Outcome {
    fn from_output(output: &Output) -> Self {
        Self {
            success: output.status.success(),
            stdout: String::from_utf8_lossy(&output.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&output.stderr).into_owned(),
        }
    }

    /// Everything the command printed, for assertions that do not care about the stream.
    pub fn all_output(&self) -> String {
        format!("{}{}", self.stdout, self.stderr)
    }
}

pub fn fixture(name: &str) -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

pub fn repo_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .expect("xtask sits inside the repo root")
        .to_path_buf()
}

/// Copies a fixture to a fresh scratch folder so that builds never share a target folder.
pub fn scratch_copy(name: &str) -> PathBuf {
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if scratch.exists() {
        fs::remove_dir_all(&scratch).expect("remove an old scratch copy");
    }
    copy_tree(&fixture(name), &scratch);
    scratch
}

pub fn empty_scratch(name: &str) -> PathBuf {
    let scratch = Path::new(env!("CARGO_TARGET_TMPDIR")).join(name);
    if scratch.exists() {
        fs::remove_dir_all(&scratch).expect("remove an old scratch folder");
    }
    fs::create_dir_all(&scratch).expect("create the scratch folder");
    scratch
}

fn copy_tree(from: &Path, to: &Path) {
    fs::create_dir_all(to).expect("create a scratch folder");
    for entry in fs::read_dir(from).expect("read a fixture folder") {
        let entry = entry.expect("read a fixture entry");
        let target = to.join(entry.file_name());
        if entry.file_type().expect("read a file type").is_dir() {
            copy_tree(&entry.path(), &target);
        } else {
            fs::copy(entry.path(), &target).expect("copy a fixture file");
        }
    }
}

fn limit_cargo(command: &mut Command) -> &mut Command {
    for var in INHERITED_BUILD_VARS {
        command.env_remove(var);
    }
    command.env("CARGO_BUILD_JOBS", CARGO_JOBS)
}

pub fn run_xtask(dir: &Path, args: &[&str]) -> Outcome {
    let mut command = Command::new(env!("CARGO_BIN_EXE_xtask"));
    limit_cargo(&mut command).current_dir(dir).args(args);
    Outcome::from_output(&command.output().expect("run the xtask binary"))
}

pub fn run_check_red(dir: &Path) -> Outcome {
    let script = repo_root().join("scripts/check-red.sh");
    let mut command = Command::new("bash");
    limit_cargo(&mut command).current_dir(dir).arg(script);
    Outcome::from_output(&command.output().expect("run scripts/check-red.sh"))
}

pub fn run_git(dir: &Path, args: &[&str]) {
    let output = Command::new("git")
        .args([
            "-c",
            "user.name=fixture",
            "-c",
            "user.email=fixture@example.com",
        ])
        .args(["-c", "commit.gpgsign=false"])
        .current_dir(dir)
        .args(args)
        .output()
        .expect("run git");
    assert!(
        output.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&output.stderr)
    );
}
