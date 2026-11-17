# cp-sat-rs

A Rust port of the CP-SAT solver from [Google OR-Tools](https://github.com/google/or-tools). The goal is full feature parity with the C++ solver, with the same `CpModelProto` and `SatParameters` input and the same `CpSolverResponse` output.

## State

Phase 0 (foundation). There is no solver yet. The repo holds the base for the port: the licence, the wire types, the file map and the first ported files.

## Upstream

The port follows `google/or-tools` `main` at commit `c931f5f778c88050a3f71d72dfc30d3d020aa821` (16/11/2026).

## Licence

Apache 2.0. See `LICENSE` and `NOTICE`. This project is a derivative of OR-Tools and keeps Google's copyright notices.
