# Corpus of the differential run

The repository holds no model file. The binary `difftest` reads its models at
run time from two upstream test sources, in the OR-Tools checkout that
`scripts/fetch-upstream.sh` places in `upstream/`:

- `upstream/ortools/sat/cp_model_solver_test.cc`
- `upstream/ortools/sat/cp_model_checker_test.cc`

Every `R"pb(...)pb"` raw string in them is a block of protobuf text. A raw string
with another delimiter is not read.

## What runs

```sh
REFERENCE_PREFIX=<prefix of the reference build> \
  cargo run --release -p cpsat-difftest --features reference --bin difftest
```

A block that the reference does not parse as a `CpModelProto`, such as a
parameter block, is skipped and counted, and the line of the run shows the error
of the reference. The Rust `parse_test_proto::<CpModelProto>` must reject the same
text: if it parses it, that is a parse disagreement. In the first run the
reference rejected 4 blocks (a field `maximize` that does not exist, and
`tightened_variables`), and Rust rejected the same 4. For every other block the run makes four
comparisons, each against the reference at the pinned commit:

| Comparison | Rust side | Reference side |
|---|---|---|
| parse | `parse_test_proto` gives the same message, and the same encoded bytes | `reference_parse_text` |
| print | `protobuf_short_debug_string` of the message | `reference_debug_string`, short form |
| validate | `validate_cp_model`, and `validate_input_cp_model` with default parameters | `reference_validate`, `reference_validate_input` |
| check | `solution_is_feasible` accepts the solution, and gives the same verdict as the reference on the solution with one variable changed | `reference_solve`, `reference_solution_is_feasible` |

The check runs for a model that both validations accept. The reference solves it
with one worker, the seed 1 and `max_deterministic_time`, and no wall clock
limit. When the answer holds a solution, one variable at a time takes another
value of its domain, for each of the first 20 variables. The summary counts the
changed solutions and how many the reference rejects, and the run fails below the
two limits that follow, so a checker that accepts every solution fails and the
check cannot disappear unnoticed. A reference answer of `MODEL_INVALID` for a
model both validations accept is a disagreement, and so is a `FEASIBLE` or
`OPTIMAL` answer that holds no solution values for a model with variables.

A panic in a Rust call is caught and reported as a disagreement for that model,
and the run goes on. The comparison has no rule that excuses a difference.

## Limits

The run exits with a failure code on any disagreement, and on these limits. They
come from the first full run, on the reference at the commit in `upstream.lock`:
204 blocks, 154 in the solver test and 50 in the checker test; 4 skipped; 200
compared; 41 invalid; 125 `OPTIMAL`, 34 `INFEASIBLE`, no `FEASIBLE`, no
`UNKNOWN`; 443 changed solutions, 246 of them rejected by the reference.

| Constant | Value | How it was set |
|---|---|---|
| `MAX_DETERMINISTIC_TIME` | 0.01 seconds | The number of `UNKNOWN` was 19 at 0.0001, 5 at 0.001, and 0 at 0.01, 0.1, 1 and 10. 0.01 is the smallest value after which it stops falling. |
| `MAX_UNKNOWN` | 1 | One model above the 0 that ended `UNKNOWN` at 0.01. The run fails when more end `UNKNOWN`. |
| `MIN_COMPARED` | 200 | The count of compared models of that run. A run that compares fewer, or none, fails. |
| `MIN_CHANGED` | 443 | The count of changed solutions of that run. The run fails below it. |
| `MIN_REJECTED` | 246 | The count of changed solutions that the reference rejects in that run. The run fails below it. |
