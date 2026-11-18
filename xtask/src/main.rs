//! Repo tool. `cargo xtask port-map` writes the port map, `--check` holds the test counts.

mod baseline;
mod cargo_list;
mod check;
mod graph;
mod phases;
mod port_map;
mod render;
mod rust_tree;
mod upstream;

use std::env;
use std::path::PathBuf;
use std::process::ExitCode;

use check::CheckOptions;

const COMMAND: &str = "port-map";
const DEFAULT_UPSTREAM_DIR: &str = "upstream/ortools";
const DEFAULT_OUT: &str = "docs/port-map.md";
const USAGE: &str = "usage: cargo xtask port-map [--upstream-dir <dir>] [--root <dir>] [--out <file>]\n       \
     cargo xtask port-map --check [--release] [--upstream] [--base <ref>] [--root <dir>] \
     [--upstream-dir <dir>]";

#[derive(Default)]
struct Flags {
    upstream_dir: Option<PathBuf>,
    root: Option<PathBuf>,
    out: Option<PathBuf>,
    base: Option<String>,
    check: bool,
    release: bool,
    upstream: bool,
}

fn parse_flags(args: impl Iterator<Item = String>) -> Result<Flags, String> {
    let mut flags = Flags::default();
    let mut args = args;
    while let Some(arg) = args.next() {
        let mut value = |name: &str| args.next().ok_or_else(|| format!("{name} needs a value"));
        match arg.as_str() {
            "--upstream-dir" => flags.upstream_dir = Some(value("--upstream-dir")?.into()),
            "--root" => flags.root = Some(value("--root")?.into()),
            "--out" => flags.out = Some(value("--out")?.into()),
            "--base" => flags.base = Some(value("--base")?),
            "--check" => flags.check = true,
            "--release" => flags.release = true,
            "--upstream" => flags.upstream = true,
            other => return Err(format!("unknown argument `{other}`")),
        }
    }
    Ok(flags)
}

fn run() -> Result<bool, String> {
    let mut args = env::args().skip(1);
    if args.next().as_deref() != Some(COMMAND) {
        return Err(format!("expected the command `{COMMAND}`"));
    }
    let flags = parse_flags(args)?;
    let root = match flags.root {
        Some(root) => root,
        None => env::current_dir()
            .map_err(|error| format!("cannot read the current folder: {error}"))?,
    };
    let upstream_dir = flags
        .upstream_dir
        .unwrap_or_else(|| root.join(DEFAULT_UPSTREAM_DIR));

    if !flags.check {
        let out = flags.out.unwrap_or_else(|| root.join(DEFAULT_OUT));
        port_map::generate(&upstream_dir, &root, &out)?;
        return Ok(true);
    }

    let options = CheckOptions {
        root,
        release: flags.release,
        upstream_dir: flags.upstream.then_some(upstream_dir),
        base: flags.base,
    };
    let problems = check::run(&options)?;
    for problem in &problems {
        eprintln!("error: {problem}");
    }
    if problems.is_empty() {
        println!("port-map: check passed");
    } else {
        eprintln!("port-map: check failed with {} problems", problems.len());
    }
    Ok(problems.is_empty())
}

fn main() -> ExitCode {
    match run() {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::FAILURE,
        Err(message) => {
            eprintln!("error: {message}\n{USAGE}");
            ExitCode::from(2)
        }
    }
}
