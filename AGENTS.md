# AGENTS.md

Rules for AI coding agents and people working in this repo. `CLAUDE.md` points here.

## What this is

A Rust port of the CP-SAT solver from Google OR-Tools, at commit `c931f5f778c88050a3f71d72dfc30d3d020aa821` of `google/or-tools`. The plan is phased: see `docs/roadmap.md`. The file map and status of every upstream file is `docs/port-map.md`. The translation rules are `docs/port-rules.md`.

## Git and pull requests

- Never commit on `main`. The first commit was the only exception.
- One pull request per change. No issue is needed first.
- Signed Conventional Commits: `git commit --gpg-sign --signoff`.
- No AI attribution anywhere: no `Co-authored-by`, no generated-by line, no session link.
- Every pull request gets a review by a separate review agent before it merges.
- Squash merges only. Never rebase, never force push, never delete a remote branch.
- Open drafts with `playbook pr create`.
- One upstream file per pull request. A file over 2,000 lines of `.h` and `.cc`, tests not counted, is split by function group across pull requests. A pull request may also be split when the port and its test file together pass 2,000 lines.

## Content

- The repo is public. Put nothing from the private `BanksiaFoundation/engine` or `engine-test-data` repos in it: no model dumps, no test rosters, no kernel code, no build script.
- The port is a derivative of OR-Tools. Keep the Apache 2.0 `LICENSE`, Google's copyright notices and the statement of changes in `NOTICE`.

## Port rules

- Mirror layout until gate B: one crate per C++ folder, one module per C++ file, no refactor of ported code.
- Every ported file starts with the upstream Apache 2.0 header, then one line: `// Ported from ortools/<path> at commit c931f5f7. Changed: translated from C++ to Rust.`
- A test that cannot compile yet is one line at its upstream position: `// PENDING(phase N): TEST(Suite, Name), needs <upstream file>`.
- A test that compiles but is red because its code belongs to a later phase carries `#[ignore = "phase N: <upstream file>"]`. The stub body is `unimplemented!("phase N: <upstream file>")`.
- `port-baseline.toml` holds the test counts per ported file. `cargo xtask port-map --check` enforces it.
- A red step stays local. A pull request holds the green state.
- No profile sets `panic = "abort"`. Release builds keep `overflow-checks = true`.

## Writing

- Simple English, active voice, no em or en dashes. No hard wrap in markdown.
- In inputs and outputs of the solver, say "input" and "output", not "request" and "response".
- First person on GitHub. Never write about the repo owner in the third person. No commit hashes in GitHub text.

## Build and test

- Cargo jobs run with `-j 4`.
- Run one solve at a time.
- GitHub Actions cannot start jobs for now. Run `cargo fmt --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace` and `cargo test --workspace --release` locally before a merge.
