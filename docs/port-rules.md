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

## 5. Shared solver state

Upstream, every component is fetched from one `Model` by its type and keeps raw pointers to other components for its whole life. Components call each other while one of them is running: `SatSolver::Propagate` calls `IntegerTrail::Propagate`, which calls back `SatSolver::NotifyThatModelIsUnsat`. The Rust form is `crates/sat/src/model.rs`. It is proven on a small copy of the SAT core, the tests of that file, and is to be confirmed by the first real file of phase 1.

The form, in short:

- The model owns each component in a heap box that never moves, and `get_or_create`, `create`, `take_ownership` and `get_singleton` give a `Handle<T>`. A handle derefs to `&T`. No `&mut` of a component is ever made.
- Every method of a component takes `&self`. A field that a method changes after `new` is a `Cell` when its type is `Copy`, else a `DebugRefCell`. A field set in `new` and never changed is a plain field.
- A `DebugRefCell` borrow lives for one statement, or for a block that makes no call that may come back to the same field. A debug build panics when two borrows overlap. A release build does not check, and an overlap is undefined behaviour.
- A model is built pinned in a box (`Model::default()` and `Model::new(name)` give a `Pin<Box<Model>>`; this is the one exception to rule 22, since a type that must not move has no `Default` impl) and drops its components in reverse order of creation.

Each case below has a C++ line and its Rust form.

### 5.1 A member pointer to another component

A member that points to a component is a `Handle<T>`, filled in `new` with `get_or_create`. A call through it is a method call on the handle.

```cpp
trail_(model->GetOrCreate<Trail>()),     // sat/integer.h:554
Trail* trail_;                           // sat/integer.h:1306
trail_->Enqueue(literal, propagator_id_);   // sat/integer.cc:1586
```

```rust
trail: model.get_or_create::<Trail>(),
trail: Handle<Trail>,
self.trail.enqueue(literal, self.propagator_id);
```

### 5.2 A component passed as an argument and also reached through a member

A `T*` argument is `&T`. The argument and the member are two shared references to one object, which is allowed. Port each use as it is upstream: the argument where upstream uses the argument, the member where upstream uses the member.

```cpp
bool IntegerTrail::Propagate(Trail* trail) {         // sat/integer.cc:736
  trail_->EnqueueWithUnitReason(lit);                // sat/integer.cc:782
  while (propagation_trail_index_ < trail->Index()) {   // sat/integer.cc:789
```

```rust
fn propagate(&self, trail: &Trail) -> bool {
    self.trail.enqueue_with_unit_reason(lit);
    while self.propagation_trail_index.get() < trail.index() {
```

### 5.3 A call back into the caller

The caller is still running, so the state the callee changes is in a `Cell` or a `DebugRefCell` that the caller does not hold borrowed across the call.

```cpp
sat_solver_->NotifyThatModelIsUnsat();          // sat/integer.cc:770
void NotifyThatModelIsUnsat() { model_is_unsat_ = true; }   // sat/sat_solver.h:508
watcher_->CallAgainDuringThisPropagation();     // sat/linear_programming_constraint.cc:2712
```

```rust
self.sat_solver.notify_that_model_is_unsat();
pub fn notify_that_model_is_unsat(&self) { self.model_is_unsat.set(true); }
self.watcher.call_again_during_this_propagation();
```

### 5.4 A pointer to a field of another component

The field is a `DebugRefCell` and the pointer is `Handle::field`, made in `after_create` (5.8) since it needs the address of the component. The other component keeps a `Handle` of the field and borrows it for one statement.

```cpp
integer_trail_->RegisterWatcher(&modified_vars_);   // sat/integer.cc:2512
std::vector<SparseBitset<IntegerVariable>*> watchers_;   // sat/integer.h:1294
bitset->Set(var);                                   // sat/integer.cc:1799
```

```rust
integer_trail.register_watcher(this.field(|watcher| &watcher.modified_vars));
watchers: DebugRefCell<Vec<Handle<DebugRefCell<SparseBitset<IntegerVariable>>>>>,
bitset.borrow_mut().set(var);
```

### 5.5 `Model* model_` as a member

The member is a `Handle<Model>`, made with `model.handle()`.

```cpp
Model* model_;   // sat/sat_solver.h:837
```

```rust
model: Handle<Model>,
// in new:
model: model.handle(),
```

### 5.6 A `const SatParameters&` member

A reference member to a component is a `Handle` too, read through its getters (rule 9).

```cpp
parameters_(*model->GetOrCreate<SatParameters>())   // sat/integer.h:557
const SatParameters& parameters_;                     // sat/integer.h:1309
```

```rust
parameters: model.get_or_create::<SatParameters>(),
parameters: Handle<SatParameters>,
```

### 5.7 A reference into another component held across a loop

The method that hands out the reference returns the `DebugRef` guard, and the caller holds it for the loop. The loop must make no call that writes the same field, as upstream, where such a call would move the data.

```cpp
const IntegerValue* lower_bounds = integer_trail_->LowerBoundsData();   // sat/linear_propagation.cc:607
const IntegerValue* LowerBoundsData() const { return var_lbs_.data(); }  // sat/integer.h:658
```

```rust
let lower_bounds = self.integer_trail.lower_bounds_data();
pub fn lower_bounds_data(&self) -> DebugRef<'_, Vec<IntegerValue>> { self.var_lbs.borrow() }
```

### 5.8 Construction order and the step after creation

`Component::new(model: &Model)` is the constructor `T(Model* model)`, or `T()` when it ignores the model. It may call `model.get_or_create` for the components it needs, so those are created first. A constructor body that hands `this` to another component does that in `Component::after_create`, which runs once the object has its address and before the model records it, so the order of the cleanup list is the upstream order. `create` runs the same steps as `get_or_create`. The model drops components in reverse order of creation, so a `Drop` impl may use the handles a component got in `new`, never one it got later.

```cpp
model->GetOrCreate<SatSolver>()->AddPropagator(this);   // sat/integer.h:558
```

```rust
fn after_create(this: &Handle<Self>, model: &Model) {
    model.get_or_create::<SatSolver>().add_propagator(handle_as_dyn!(this, dyn SatPropagator));
}
```

### 5.9 Propagator lists of trait objects

A list of pointers to an interface is a `DebugRefCell<Vec<Handle<dyn Trait>>>`. `handle_as_dyn!` turns a `Handle<T>` into a `Handle<dyn Trait>`. The loop that calls them clones one handle per statement and does not hold the list borrowed during the call, since the callee may come back to the list owner.

```cpp
std::vector<SatPropagator*> propagators_;          // sat/sat_solver.h:850
virtual bool Propagate(Trail* trail) = 0;          // sat/sat_trail.h:513
if (!propagator->Propagate(trail_)) return false;  // sat/sat_solver.cc:2299
```

```rust
propagators: DebugRefCell<Vec<Handle<dyn SatPropagator>>>,
fn propagate(&self, trail: &Trail) -> bool;
let propagator = self.propagators.borrow()[i].clone();
if !propagator.propagate(&self.trail) { return false; }
```

### 5.10 Worker threads

A model is not `Send`. Upstream builds the model of a full worker in its constructor (`sat/cp_model_solver.cc:1113`), which the main thread calls (`sat/cp_model_solver.cc:2154`), and runs it in the task of `GenerateTask` (`sat/cp_model_solver.cc:1193`) on a thread of a pool. In Rust, a worker builds its own model on the thread that runs it and keeps it there. An object that workers share is an `Arc<T>` with `T: Send + Sync`, and each worker registers a clone in its own model.

```cpp
local_model->Register<SharedResponseManager>(response);   // sat/cp_model_solver_helpers.cc:2254
```

```rust
local_model.register(response.clone());   // response: Arc<SharedResponseManager>
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

A `TYPED_TEST` becomes one Rust test per type of the upstream type list. The body is one generic function over the type, and a macro makes the tests, so each has its own name and its own pass or fail. The macro takes the test names as well as the types, since a macro cannot join identifiers. Each name starts with the name rule 15 gives the typed test.

```cpp
TYPED_TEST(BitIndex, SingleBit) {   // util/bitset_test.cc:132, 10 types
```

```rust
fn bit_index_single_bit<T: TestCase>() { /* body */ }

typed_test!(
    bit_index_single_bit,
    bit_index_single_bit_lsb_32_basic => Lsb32Basic,
    bit_index_single_bit_lsb_32_de_bruijn => Lsb32DeBruijn,
    /* the other 8 types */
);
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

A C++ template that takes a strong integer type (`IntegerType` with `value()`) is a generic function bounded by a trait. The trait is `StrongInt64` of `crates/util/src/saturated_arithmetic.rs`, the first file that needs one. The macro `define_strong_int64_type!` implements it for each type it defines, and later files reuse it.

```cpp
template <typename IntegerType>
bool SafeAddInto(IntegerType a, IntegerType* b);   // util/saturated_arithmetic.h:120
```

```rust
pub fn safe_add_into<T: StrongInt64>(a: T, b: &mut T) -> bool {
```

## 17. Overloads and templates on primitive types

A C++ function template that is specialised on primitive types, that branches on its type with `if constexpr`, or an overload set, is a trait implemented for each type, with a free function that calls it.

```cpp
template <typename T> T CapOrFloatAdd(T x, T y);   // util/saturated_arithmetic.h:219
```

```rust
pub fn cap_or_float_add<T: CapOrFloatAdd>(x: T, y: T) -> T {
```

## 18. Operators of strong integer types

The macros of rule 4 give each type the operators of the C++ class, with the type or its integer on either side, except that a compound assignment takes the type or the integer on the right only, never an integer on the left (`util/strong_integers.h:104`). `<<` on a strong type keeps the bits that fit, as C++20 does for signed types, and panics only for a shift amount at or above the bit width. A C++ operator that has no Rust operator is a method: `x++` is `x.post_increment()`, `++x` is `x.pre_increment()`, `x--` and `--x` likewise, and `+x` is `x.unary_plus()`. `numeric_limits<T>::min()`, `lowest()` and `max()` are the constants `T::MIN` and `T::MAX`. The accessor `value<ValType>()` is `value()` and an `as` cast.

```cpp
const ThisType operator++(int);   // util/strong_integers.h:114
```

```rust
pub fn post_increment(&mut self) -> Self
```

## 19. Overloads and names that collide

A C++ overload set whose members take different parameter lists keeps the name for the first member in header order. Each later member adds a suffix that names what is new in its parameters. Two C++ names that differ only by case, such as `Resize` and `resize`, get the same treatment: the one with the odd parameter type takes the suffix.

```cpp
int64_t LeastSignificantBitPosition64(const uint64_t* bitset, uint64_t start, uint64_t end);   // util/bitset.h:261
```

```rust
pub fn least_significant_bit_position64_in_range(bitset: &[u64], start: u64, end: u64) -> i64
```

## 20. Operators that have no operator in Rust

`operator[]` that returns a `bool` is the `Index` trait with `Output = bool`, which hands out a reference to a constant `true` or `false`. `operator*` of an iterator is a method `current()`, and the iterator also implements `Iterator`. `begin()` and `end()` stay as methods.

```cpp
bool operator[](IndexType index) const { return IsSet(index); }   // util/bitset.h:383
```

```rust
impl<I: BitsetIndex> Index<I> for Bitset64<I> {
    type Output = bool;
    fn index(&self, index: I) -> &bool { if self.is_set(index) { &true } else { &false } }
}
```

## 21. Template arguments that are enum values

Stable Rust has no enum const generics, so a C++ template argument that is an enum value is a function argument in Rust. A `static_assert` on such an argument becomes a `panic!` at run time.

```cpp
template <BitScanAlgorithm algo = BitScanAlgorithm::kStd, typename T>
inline int LeastSignificantBitPosition(T n);   // util/bitset.h:90
```

```rust
pub fn least_significant_bit_position<T: BitScanWord>(algo: BitScanAlgorithm, n: T) -> i32
```

## 22. Constructors

A C++ default constructor is `Default`. The constructor with the most parameters is `new`. Every other constructor of the overload set takes a suffix that names its parameter, as rule 19 says: `new_value` for a single value, `new_from_intervals` for a vector, `_i32` for the same form over `int`.

```cpp
explicit Domain(int64_t value);       // util/sorted_interval_list.h:142
Domain(int64_t left, int64_t right);  // util/sorted_interval_list.h:148
```

```rust
pub fn new_value(value: i64) -> Self
pub fn new(left: i64, right: i64) -> Self
```

## 23. Nested types, optional arguments and iterators

A type nested in another keeps both names joined: `ClosedInterval::Iterator` is `ClosedIntervalIterator`. A default argument is an `Option` that the caller writes: `exact = nullptr` is `exact: Option<&mut bool>` and `None`. `operator<` with no `operator==` of the same fields is `PartialOrd`; the `Ord` trait is left out when its `min` and `max` would hide a method of the same name. A C++ `begin()` and `end()` pair for a range `for` is `IntoIterator`, and `operator<<` is `Display`, so `ToString()` is `to_string()`.

```cpp
ABSL_MUST_USE_RESULT Domain MultiplicationBy(int64_t coeff, bool* exact = nullptr) const;  // util/sorted_interval_list.h:408
```

```rust
pub fn multiplication_by(&self, coeff: i64, exact: Option<&mut bool>) -> Domain
```

## 24. `std::set` positions and `LOG(DFATAL)`

A `std::set` that hands out iterators is a sorted `Vec` and an iterator is the `usize` position, since stable Rust has no cursor on a `BTreeSet`. `LOG(DFATAL)` is `panic!` in a debug build and `log::error!` in an optimised build. `DLOG_IF(DFATAL, c)` is `debug_assert!`, which is silent in an optimised build as `DLOG_IF` is.

```cpp
auto result = intervals_.insert({start, end});   // util/sorted_interval_list.cc:975
LOG(DFATAL) << "Invalid interval: " << ...;      // util/sorted_interval_list.cc:971
```

```rust
let position = self.intervals.partition_point(|i| (i.start, i.end) < (start, end));
if cfg!(debug_assertions) { panic!("Invalid interval: {invalid}"); }
log::error!("Invalid interval: {invalid}");
```
