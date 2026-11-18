# Port rules

How a C++ file of OR-Tools becomes a Rust module in this repo. Each rule has one C++ line and its Rust form. Upstream paths are relative to `ortools/` at the commit in `upstream.lock`.

A rule here wins over personal style. When a file needs a case no rule covers, the pull request that ports the file adds the rule.

## 1. Modules and names

One crate per C++ folder and one module per C++ file: `ortools/util/bitset.{h,cc}` becomes `crates/util/src/bitset.rs`. A function keeps its name in `snake_case`. A type keeps its name. Items keep the order they have upstream, the `.h` order first.

```cpp
inline int64_t CapAdd(int64_t x, int64_t y) {   // util/saturated_arithmetic.h:222
```

```rust
pub fn cap_add(x: i64, y: i64) -> i64 {
```

## 2. File header

Every ported file starts with the upstream Apache 2.0 header, unchanged, then one line that names the source and states the change.

```cpp
// Copyright 2010-2025 Google LLC
// Licensed under the Apache License, Version 2.0 (the "License");
```

```rust
// Copyright 2010-2025 Google LLC
// Licensed under the Apache License, Version 2.0 (the "License");
// (the rest of the upstream header)
//
// Ported from ortools/util/bitset.h at commit c931f5f7. Changed: translated from C++ to Rust.
```

## 3. Integer types

`int64_t` is `i64`, `int32_t` and `int` are `i32`, `uint64_t` is `u64`, `size_t` is `usize`. An operation that saturates or wraps upstream names that in Rust: `saturating_add`, `wrapping_mul`, or a `cap_*` function of `saturated_arithmetic`. A plain `+` means the result cannot overflow. Overflow checks are on in every profile, so a plain `+` that overflows panics in a release build too.

```cpp
const int64_t result = TwosComplementAddition(x, y);   // util/saturated_arithmetic.h:163
```

```rust
let result: i64 = x.wrapping_add(y);
```

## 4. Strong integer types

A type made by `DEFINE_STRONG_INDEX_TYPE` or `DEFINE_STRONG_INT64_TYPE` is a newtype made by the Rust macro of the same name in `snake_case`, from `crates/util/src/strong_integers.rs`. It has no implicit conversion. Read the inner integer with `value()`.

```cpp
DEFINE_STRONG_INDEX_TYPE(BooleanVariable);   // sat/sat_literal.h
```

```rust
define_strong_index_type!(BooleanVariable);
```

## 5. Shared solver state (provisional)

This rule is provisional until `sat/model.h` is ported. The pull request that ports it replaces this section with the confirmed form.

Upstream, every component is fetched from one `Model` and keeps raw pointers to other components for its whole life. In Rust a component is fetched from the `Model` by its type, and one component holds a handle or an index to another, never a long-lived `&` or `&mut`. A borrow of another component lasts for one call.

```cpp
auto* trail = model->GetOrCreate<Trail>();
```

```rust
let trail = model.get_or_create::<Trail>();
```

## 6. Hash containers

| Abseil or STL | Rust |
|---|---|
| `absl::flat_hash_map<K, V>` | `std::collections::HashMap<K, V>` |
| `absl::flat_hash_set<K>` | `std::collections::HashSet<K>` |
| `absl::btree_map<K, V>`, `std::map<K, V>` | `std::collections::BTreeMap<K, V>` |
| `absl::btree_set<K>`, `std::set<K>` | `std::collections::BTreeSet<K>` |
| `std::vector<T>` | `Vec<T>` |
| `absl::Span<const T>` | `&[T]` |

Code whose result depends on the iteration order of a hash container uses an insertion-ordered container (`indexmap::IndexMap` or `IndexSet`), with a comment that names the upstream line. Upstream gets a different order on each run there, so any fixed order is a valid one.

```cpp
absl::flat_hash_set<int> self_loops;   // sat/cp_model_checker.cc:685
```

```rust
let mut self_loops: HashSet<i32> = HashSet::new();
```

## 7. Checks

`CHECK` and its forms are `assert!` and its forms. `DCHECK` and its forms are `debug_assert!` and its forms. The panic message holds the same text as the upstream message when the upstream check has one.

```cpp
DCHECK_EQ(a.size(), b.size());   // util/bitset.h:415
```

```rust
debug_assert_eq!(a.size(), b.size());
```

## 8. Death tests

Each `EXPECT_DEATH` statement is one call of `expect_death(|| ..., "message")` from `crates/testutil`. The helper catches the panic and compares the message, so a test with several death statements runs all of them.

Each `EXPECT_DEBUG_DEATH` statement is one call of `expect_debug_death(|| ..., "message")`. In a debug build it behaves like `expect_death`. In a release build it runs the closure, any inner assertion included, and requires that the closure does not panic.

A C++ `#ifndef NDEBUG` or `DEBUG_MODE` guard is the matching `cfg(debug_assertions)` attribute.

A closure works on its own copy of an object that later statements use again, because a caught panic can leave the object half changed.

`#[should_panic]` is only for a test whose single statement dies.

```cpp
EXPECT_DEBUG_DEATH(ClosedInterval(1, 0), "Invalid");   // util/sorted_interval_list_test.cc:45
```

```rust
expect_debug_death(|| { ClosedInterval::new(1, 0); }, "Invalid");
```

## 9. Proto2 defaults

`SatParameters` is proto2 and most fields declare a default. Code reads a field through the generated getter, which returns the declared default when the field is not set. Code never reads the raw `Option`.

```cpp
if (params.max_time_in_seconds() > 0) {
```

```rust
if params.max_time_in_seconds() > 0.0 {
```

## 10. Typed tests

A `TYPED_TEST` becomes one Rust test per type of the upstream type list. A macro makes them, so each has its own name and its own pass or fail.

```cpp
TYPED_TEST(BitIndex, SingleBit) {   // util/bitset_test.cc:132, 10 types
```

```rust
typed_test!(bit_index_single_bit, [Lsb32Basic, Lsb32DeBruijn, /* the other 8 types */], |T| { /* body */ });
```

## 11. `base/` headers

The port set includes eight `base/` headers. One is ported. The others are replaced.

| Header | Rust form |
|---|---|
| `base/hash.h` | Ported: `crates/base/src/hash.rs` |
| `base/types.h` | The primitive types and their constants: `kint64max` is `i64::MAX` |
| `base/stl_util.h` | Methods of `Vec` and slices: `gtl::STLSortAndRemoveDuplicates(&v)` is `v.sort_unstable(); v.dedup();` |
| `base/options.h` | Nothing. It selects C++ build options |
| `base/helpers.h` | `std::fs` and `std::io`, in the few places a ported file reads or writes a file |
| `base/base_export.h` | Nothing. `pub` states what a crate exports |
| `base/strong_vector.h` | A `Vec<T>` wrapper that takes its index as a strong index type |
| `base/log_severity.h` | The levels of the `log` crate |

```cpp
gtl::STLSortAndRemoveDuplicates(&result);   // sat/cp_model_utils.cc:788
```

```rust
result.sort_unstable();
result.dedup();
```

## 12. Logging

`VLOG(n)` is a macro of the `log` crate: `VLOG(1)` is `debug!` and `VLOG(2)` and above are `trace!`. `LOG(INFO)`, `LOG(WARNING)` and `LOG(ERROR)` are `info!`, `warn!` and `error!`. `LOG(FATAL)` is `panic!`. The solver log that a user turns on with `log_search_progress` is not this: it is ported with the solver.

```cpp
VLOG(1) << "Activity: " << sum;   // sat/cp_model_checker.cc:1462
```

```rust
log::debug!("Activity: {sum}");
```

## 13. Random numbers

Every random draw uses one generator type, `rand_chacha::ChaCha8Rng`, whatever generator the C++ line uses. A function that draws takes the generator as `&mut ChaCha8Rng`. A test builds it from a fixed seed, the upstream seed when the upstream test has one. The Rust numbers differ from the C++ numbers, so a test asserts a property and not a drawn value.

```cpp
std::mt19937 random(12345);   // util/saturated_arithmetic_test.cc:105
```

```rust
let mut random = ChaCha8Rng::seed_from_u64(12345);
```

## 14. Stubs, ignored tests, pending lines and the baseline

A body that belongs to a later phase is a stub. A test that compiles but needs a stub is ignored with the phase and the upstream file. A test that cannot compile yet is one comment line at its upstream position. `port-baseline.toml` records the counts per test file, and `cargo xtask port-map --check` fails when the tree and the baseline differ. `scripts/check-red.sh` proves each ignored test fails at a stub.

```cpp
CpSolverResponse Solve(const CpModelProto& model_proto);   // sat/cp_model_solver.h

TEST(SuiteName, CaseName) { /* body */ }
```

```rust
pub fn solve(model_proto: &CpModelProto) -> CpSolverResponse {
    unimplemented!("phase 2: sat/cp_model_solver.cc")
}

// A case that compiles and needs the stub:
#[test]
#[ignore = "phase 2: sat/cp_model_solver.cc"]
fn suite_name_case_name() { /* body */ }

// A case that cannot compile yet:
// PENDING(phase 5): TEST(SuiteName, CaseName), needs sat/synchronization.h
```

An entry of `port-baseline.toml` for one ported test file:

```toml
[[file]]
upstream = "util/bitset_test.cc"
rust = "crates/util/src/bitset.rs"
binary = "util"
module = "bitset::tests"
upstream_cases = 38
typed_additions = 18
debug_only = 3
release_only = 0
pending = 0
ignored = 0

[[file.added]]
name = "an_added_test"
reason = "why upstream has no such case"
```

## 15. Test names

The unit test of a file lives in its module as `#[cfg(test)] mod tests`, in upstream order. `TEST(Suite, Name)`, `TEST_F(Suite, Name)` and `TEST_P(Suite, Name)` become `fn suite_name` in `snake_case`. A run of capital letters stays one word: `CPModel` is `cp_model`. `cargo xtask port-map --check --upstream` matches the names, so a renamed test fails CI. A test that a macro of the test file makes counts once per use and takes the `TEST_F(Suite, Name)` header inside the macro with the arguments of the use put in: `TEST_BINARY_OPERATOR(CapAdd, "+", ReferenceCapAdd)` at `util/saturated_arithmetic_test.cc:153` makes `TEST_F(SaturatedArithmeticTest, CapAdd)`, so its Rust name is `saturated_arithmetic_test_cap_add`.

```cpp
TEST(ClosedIntervalConstructorTest, DFatalInNonOptIfStartGreaterThanEnd) {   // util/sorted_interval_list_test.cc:43
```

```rust
#[test]
fn closed_interval_constructor_test_d_fatal_in_non_opt_if_start_greater_than_end() {
```

## 16. Templates over strong integer types

A C++ template that takes a strong integer type (`IntegerType` with `value()`) is a generic function bounded by a trait of the file that needs it. A strong type implements the trait, and a test defines its own type for it. This stays until `crates/util/src/strong_integers.rs` is ported.

```cpp
template <typename IntegerType>
bool SafeAddInto(IntegerType a, IntegerType* b);   // util/saturated_arithmetic.h:117
```

```rust
pub fn safe_add_into<T: StrongInt64>(a: T, b: &mut T) -> bool {
```

## 17. Overloads and templates on primitive types

A C++ function template that is specialised on primitive types, or an overload set, is a trait implemented for each type, with a free function that calls it.

```cpp
template <typename T> T CapOrFloatAdd(T x, T y);   // util/saturated_arithmetic.h:233
```

```rust
pub fn cap_or_float_add<T: CapOrFloatAdd>(x: T, y: T) -> T {
```

## 18. Operators of strong integer types

The macros of rule 4 give each type the operators of the C++ class, with the type or its integer on either side. A C++ operator that has no Rust operator is a method: `x++` is `x.post_increment()`, `++x` is `x.pre_increment()`, `x--` and `--x` likewise, and `+x` is `x.unary_plus()`. `numeric_limits<T>::min()`, `lowest()` and `max()` are the constants `T::MIN` and `T::MAX`. The accessor `value<ValType>()` is `value()` and an `as` cast.

```cpp
const ThisType operator++(int);   // util/strong_integers.h:117
```

```rust
pub fn post_increment(&mut self) -> Self
```
