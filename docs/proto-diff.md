# Proto notes against 9.15

What changed in `ortools/sat/cp_model.proto` and `ortools/sat/sat_parameters.proto` between the OR-Tools tag `v9.15` and the commit in `upstream.lock`. It tells a caller built on the 9.15 protos what the Rust port reads differently.

I made it once, from a `git diff` of the two files between a temporary clone of tag `v9.15` and the pinned source. No tool and no 9.15 file is in the repo. To check it, clone `https://github.com/google/or-tools` at `v9.15` and diff the two files against `upstream/ortools/sat/`.

## Classes

Each field of the diff has one class.

| Class | Meaning |
|---|---|
| Removed | The field is gone from the pinned file. Its number is not reserved. |
| Number changed | The field keeps its name and gets another number. |
| Type or label changed | The field keeps its name and number and gets another type or label. |
| Added | The field is new in the pinned file. |
| Default or comment only | The field keeps its name, number, type and label. Only its default, an option or its comment changed. |

## Counts

| File | Fields in 9.15 | Fields in the pinned file | Removed | Number changed | Type or label changed | Default or comment only | Added |
|---|---|---|---|---|---|---|---|
| `cp_model.proto` | 141 | 145 | 0 | 0 | 0 | 3 | 4 |
| `sat_parameters.proto` | 303 | 323 | 5 | 0 | 0 | 4 | 25 |

That is 12 existing fields that changed: 3 in `cp_model.proto` and 9 in `sat_parameters.proto`. A count of the changed diff lines that carry a field number gives 11, because the new default of `shaving_deterministic_time_in_probing_search` sits on a wrapped line without the number. `SatParameters.SearchBranching` also gains two enum values. No field changes number, type or label.

## Existing fields that changed

| Message | Field | Number | Class | What changed |
|---|---|---|---|---|
| `InverseConstraintProto` | `f_direct` | 1 | Default or comment only | The comment `// Legacy field.` is new. |
| `InverseConstraintProto` | `f_inverse` | 2 | Default or comment only | The comment `// Legacy field.` is new. |
| `CpObjectiveProto` | `scaling_was_exact` | 6 | Default or comment only | The option `[deprecated = true]` is new. |
| `SatParameters` | `inprocessing_minimization_use_all_orderings` | 298 | Removed | Default was `false`. |
| `SatParameters` | `violation_ls_compound_move_probability` | 259 | Removed | Default was `0.5`. |
| `SatParameters` | `check_drat_proof` | 346 | Removed | Default was `false`. |
| `SatParameters` | `output_drat_proof` | 347 | Removed | Default was `false`. |
| `SatParameters` | `max_drat_time_in_seconds` | 348 | Removed | Default was `inf`. |
| `SatParameters` | `use_new_integer_conflict_resolution` | 336 | Default or comment only | Default goes from `false` to `true`. |
| `SatParameters` | `mip_check_precision` | 128 | Default or comment only | Default goes from `1e-4` to `1e-5`. |
| `SatParameters` | `mip_max_valid_magnitude` | 199 | Default or comment only | Default goes from `1e20` to `1e25`. |
| `SatParameters` | `shaving_deterministic_time_in_probing_search` | 204 | Default or comment only | Default goes from `0.001` to `0.05`. |

## Fields added

In `cp_model.proto`:

| Message | Field | Type | Number |
|---|---|---|---|
| `InverseConstraintProto` | `f_expr_direct` | `repeated LinearExpressionProto` | 3 |
| `InverseConstraintProto` | `f_expr_inverse` | `repeated LinearExpressionProto` | 4 |
| `FloatObjectiveProto` | `domain_mins` | `repeated int64` | 5 |
| `FloatObjectiveProto` | `domain_maxs` | `repeated int64` | 6 |

In `sat_parameters.proto`, all in `SatParameters` and all `optional`, with their defaults:

| Field | Type | Number | Default |
|---|---|---|---|
| `num_ordering_in_vivification` | `int32` | 356 | `1` |
| `detect_encoded_complex_domain` | `bool` | 357 | `true` |
| `mip_presolve_restrict_bounds` | `bool` | 359 | `true` |
| `cp_model_pure_sat_presolve` | `bool` | 360 | `false` |
| `inprocessing_use_transitive_reduction` | `bool` | 361 | `true` |
| `inprocessing_detect_and_sweep_circuit` | `bool` | 362 | `false` |
| `try_hint_as_assumptions` | `bool` | 363 | `true` |
| `learn_on_assumptions_unsat` | `bool` | 364 | `true` |
| `shared_tree_worker_propagate_intermediate_nodes` | `bool` | 365 | `false` |
| `shaving_search_use_random_search` | `bool` | 366 | `false` |
| `shaving_search_restart_limit` | `int32` | 367 | `10` |
| `objective_shaving_search_threshold` | `int64` | 368 | `1000` |
| `cp_model_direct_solve` | `bool` | 369 | `false` |
| `mip_use_tight_bounds` | `bool` | 370 | `true` |
| `mip_tight_precision` | `double` | 371 | `1e-9` |
| `mip_verify_solution` | `bool` | 372 | `false` |
| `mip_proper_relaxation` | `bool` | 373 | `false` |
| `mip_scale_for_precision` | `bool` | 374 | `false` |
| `use_boolean_equivalences_in_lp` | `bool` | 375 | `true` |
| `use_tight_upper_implied_bounds_in_cuts` | `bool` | 376 | `true` |
| `shared_tree_worker_min_dtime_before_sync` | `double` | 377 | `2.0` |
| `lns_initial_presolve_deterministic_limit` | `double` | 378 | `0.5` |
| `lns_max_presolve_deterministic_limit` | `double` | 379 | `10.0` |
| `feasibility_jump_ignore_redundant_constraints` | `bool` | 715 | `true` |
| `lns_ignore_redundant_constraints` | `bool` | 716 | `true` |

The enum `SatParameters.SearchBranching` gains `ROUND_ROBIN_SHAVING_SEARCH = 9` and `ALT_FIXED_SEARCH = 10`.

## Fields an older caller could set that the pinned commit no longer reads

| Field | Why it is not read |
|---|---|
| `SatParameters.check_drat_proof` (346) | Removed. At 9.15 `sat/cp_model_solver.cc`, `sat/cp_model_search.cc` and `sat/lrat_proof_handler.cc` read it. |
| `SatParameters.output_drat_proof` (347) | Removed. Read in the same three files at 9.15. |
| `SatParameters.max_drat_time_in_seconds` (348) | Removed. At 9.15 `sat/lrat_proof_handler.cc` read it and `sat/parameters_validation.cc` checked it is not NaN. |
| `SatParameters.inprocessing_minimization_use_all_orderings` (298) | Removed. At 9.15 `sat/vivification.cc` read it. The new `num_ordering_in_vivification` (356) is the nearest field. |
| `SatParameters.violation_ls_compound_move_probability` (259) | Removed. At 9.15 only `sat/parameters_validation.cc` read it, to check its range. |
| `CpObjectiveProto.scaling_was_exact` (6) | Kept and marked deprecated, but nothing under `ortools/` outside the generated Julia wrappers reads or sets it at the pinned commit. At 9.15 `sat/cp_model_solver.cc` read it and `sat/lp_utils.cc` set it. |

A removed field does not parse from text format any more: the parser reports an unknown field. On the wire it is skipped as an unknown field. None of the five numbers is reserved, so a later change can reuse them.

Two more changes alter a result without any field being set. `use_new_integer_conflict_resolution` is now on by default, and the defaults of `mip_check_precision`, `mip_max_valid_magnitude` and `shaving_deterministic_time_in_probing_search` moved. A caller that leaves these unset gets the new values.

`InverseConstraintProto.f_direct` and `f_inverse` are marked legacy but the pinned commit still reads them in `sat/cp_model_checker.cc`, next to the new `f_expr_direct` and `f_expr_inverse`. A model may fill one pair or the other, not both: the checker rejects a model that sets `f_inverse` and `f_expr_inverse` together.
