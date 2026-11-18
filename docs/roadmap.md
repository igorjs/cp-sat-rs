# Roadmap

The goal is a Rust solver with full feature parity with C++ CP-SAT at the commit in `upstream.lock`: the same `CpModelProto` and `SatParameters` input, the same `CpSolverResponse` output, with solution, bound and log callbacks and a stop call.

The port mirrors upstream: one crate per C++ folder and one module per C++ file. It moves in dependency order, and each ported file brings its upstream tests with it. `docs/port-map.md` shows the phase and the status of every file.

## Phases

| Phase | Name | What it ports | Ends with |
|---|---|---|---|
| 0 | Foundation | The repo base, the wire types, the reference build, the test helpers, the first `util` and `sat` files, the model checker, and the public solve calls as stubs | 222 upstream test cases green |
| 1 | SAT core | Clauses, propagation, conflict analysis and restarts | Pure Boolean models solve |
| 2 | Integer layer and single thread search | Integer variables, linear and scheduling propagators, one search worker | Gate A |
| 3 | Presolve | Model reductions before search and the solution mapping back | Same answers with presolve on |
| 4 | LP and cuts | The simplex code in `glop`, the linear relaxation and cut generators | Same bounds as upstream on LP heavy models |
| 5 | Search workers and parallel | The worker portfolio, shared state between workers, large neighbourhood search | Gate B, then gate C |
| 6 | Other constraint families and proof logging | Every constraint kind and parameter not ported before, and proof output | Full parity |

Each phase gets its own plan before work starts. A phase ports the upstream tests of its files. The tests in `crates/sat/tests/cp_model_solver_test.rs` are written in phase 0 and stay ignored until the phase that ports what each one needs.

## Gates

A gate is a measured result on real workloads of the scheduling engine this solver is built for. The gates come in order.

| Gate | After phase | Condition |
|---|---|---|
| A | 2 | On every input of the target workload the Rust solver returns a solution that an independent evaluator accepts. On small inputs that CP-SAT proves optimal, it reaches the same objective value. |
| B | 5 | On the target workload no input gets a worse status than with CP-SAT, the time to the first solution stays within 1.5 times, and the objective value is equal or better on at least 90% of inputs. |
| C | 5 | A measured win over CP-SAT on one named workload. |

Until gate B the port stays a mirror of upstream, with no refactor of ported code. A design that differs from upstream is allowed only after gate B, and only with a measurement.

## Reference

The C++ solver at the pinned commit is the reference at every phase. `scripts/build-reference.sh` builds it, and `crates/difftest` compares the Rust code with it.
