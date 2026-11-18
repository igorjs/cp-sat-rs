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

`int64_t` is `i64`, `int32_t` and `int` are `i32`, `uint64_t` is `u64`, `size_t` is `usize`. An operation that saturates or wraps upstream names that in Rust: `saturating_add`, `wrapping_mul`, or a `cap_*` function of `saturated_arithmetic`. A plain `+` means the result cannot overflow. Overflow checks are on in every profile, so a plain `+` that overflows panics in a release build too. Where upstream signed arithmetic overflows for a reachable input, the port takes the value an optimised build gives, with a `wrapping_*` call and a comment.

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

A type nested in another keeps both names joined: `ClosedInterval::Iterator` is `ClosedIntervalIterator`. A default argument is an `Option` that the caller writes: `exact = nullptr` is `exact: Option<&mut bool>` and `None`. `operator<` is `PartialOrd` when `operator==` compares the same fields. When they differ, as `ClosedInterval::operator<` (start only) and `operator==` (start and end) do, there is no `PartialOrd`, which would break the trait contract: the `operator<` is a named method, `less_than`, and the order is compared in place. The `Ord` trait is left out when its `min` and `max` would hide a method of the same name. A C++ `begin()` and `end()` pair for a range `for` is `IntoIterator`, and `operator<<` is `Display`, so `ToString()` is `to_string()`.

```cpp
ABSL_MUST_USE_RESULT Domain MultiplicationBy(int64_t coeff, bool* exact = nullptr) const;  // util/sorted_interval_list.h:408
```

```rust
pub fn multiplication_by(&self, coeff: i64, exact: Option<&mut bool>) -> Domain
```

## 24. `std::set` positions and `LOG(DFATAL)`

A `std::set` or an `absl::btree_set` that hands out iterators is a sorted `Vec` and an iterator is the `usize` position, since stable Rust has no cursor on a `BTreeSet`. `LOG(DFATAL)` is `panic!` in a debug build and `log::error!` in an optimised build. `DLOG_IF(DFATAL, c)` is `debug_assert!`, which is silent in an optimised build as `DLOG_IF` is.

```cpp
auto result = intervals_.insert({start, end});   // util/sorted_interval_list.cc:975
LOG(DFATAL) << "Invalid interval: " << ...;      // util/sorted_interval_list.cc:971
```

```rust
let position = self.intervals.partition_point(|i| (i.start, i.end) < (start, end));
if cfg!(debug_assertions) { panic!("Invalid interval: {invalid}"); }
log::error!("Invalid interval: {invalid}");
```

## 25. Constants and stream operators on spans

A C++ constant `kName` is a `pub const` named `K_NAME`, as the local constants of the ported files are. A strong type made by the macros of rule 4 has `const fn new`, so such a constant is built with it. An `operator<<` on `absl::Span<const T>` has no `Display` form in Rust, since the trait cannot be implemented for a slice: it is a free function that returns the `String`.

```cpp
const LiteralIndex kNoLiteralIndex(-1);   // sat/sat_literal.h:40
inline std::ostream& operator<<(std::ostream& os, absl::Span<const Literal> literals);   // sat/sat_literal.h:123
```

```rust
pub const K_NO_LITERAL_INDEX: LiteralIndex = LiteralIndex::new(-1);
pub fn format_literals(literals: &[Literal]) -> String
```

## 26. Protobuf messages

A generated C++ message is the `prost` struct of `cpsat-proto`. A repeated field is a `Vec` and `x_size()` is `x.len()`. A singular message field is an `Option`: `has_x()` is `x.is_some()`, the const getter of an unset field is the empty default message. A reader skips an unset field only when the empty message adds nothing: `FingerprintExpression` (`sat/cp_model_utils.cc:1095`) hashes `lin.offset()` even for an empty expression, so a reader of an unset expression must still hash the default message, and `mutable_x()` is `x.get_or_insert_default()`, which creates the field as C++ does. A `oneof` is the `Option` of the generated enum, matched in the order of the C++ `switch`. `Clear()` is `*message = Default::default()`.

```cpp
if (proto->has_target()) f(proto->mutable_target());   // sat/cp_model_utils.cc:538
for (int& r : *ct->mutable_enforcement_literal()) f(&r);   // sat/cp_model_utils.cc:268
```

```rust
if let Some(target) = &mut proto.target { f(target); }
for r in &mut ct.enforcement_literal { f(r); }
```

## 27. Function references and default values

`absl::FunctionRef<void(T)>` is a generic parameter `impl FnMut(T)`, taken by value. A default argument that has a value is an explicit parameter, and the caller writes the default. A C++ name that is a Rust keyword takes a trailing underscore (`Const` is `const_`).

```cpp
int64_t LinearExpressionGcd(const LinearExpressionProto& expr, int64_t gcd = 0);   // sat/cp_model_utils.h:169
static constexpr AffineExpr Const(int64_t cst);   // sat/cp_model_utils.h:122
```

```rust
pub fn linear_expression_gcd(expr: &LinearExpressionProto, gcd: i64) -> i64
pub const fn const_(cst: i64) -> AffineExpr
```

## 28. Flags

An `ABSL_FLAG` is a `pub static` named in `SCREAMING_SNAKE_CASE` with the prefix of the flag kept: an `AtomicBool` for a bool, and a `LazyLock<Mutex<String>>` for a string with a non-const default. Nothing parses a command line in the port, so a flag changes only when code stores to it.

```cpp
ABSL_FLAG(bool, cp_model_dump_models, false, "...");   // sat/cp_model_utils.cc:45
```

```rust
pub static CP_MODEL_DUMP_MODELS: AtomicBool = AtomicBool::new(false);
```

## 29. Staged ports of a long file

A file whose header and source together pass 2,000 lines goes out in several pull requests, one for each function group. A test case whose function belongs to a later group is one `// PENDING(phase 1): TEST(Suite, Name), needs the Nth group of <upstream file>` line at its upstream position, and the baseline counts it in `pending`. The phase is the phase after the current one, because the check rejects a pending line at or below the current phase; the last pull request of the file leaves no pending line, and no stub stands in for a later function.

## 30. Templates over generated messages and `mutable` fields

A C++ template over message types that share a field (`ProtoWithDomain`) is a trait with the accessors the template calls, implemented for each message type the code uses. A C++ `const` method that changes `mutable` fields takes `&mut self` in Rust, which the caller already holds.

```cpp
template <typename ProtoWithDomain>
bool DomainInProtoContains(const ProtoWithDomain& proto, int64_t value);   // sat/cp_model_utils.h:244
std::pair<double, double> TightBounds() const;   // util/full_precision_inequalities.h:171
```

```rust
pub fn domain_in_proto_contains<P: ProtoWithDomain>(proto: &P, value: i64) -> bool
pub fn tight_bounds(&mut self) -> (f64, f64)
```

## 31. Raw bytes of a field, and the case enum of a `oneof`

A C++ `reinterpret_cast<const char*>` over a field or a repeated field, to hash its memory, is a trait that appends the native-endian bytes of each element. A C++ `oneof` case enum is a Rust enum of the same cases, numbered by the field numbers, since the generated Rust code holds the `oneof` as an `Option` and has no such enum.

```cpp
fasthash64(reinterpret_cast<const char*>(&field), sizeof(T), seed);   // sat/cp_model_utils.h:499
ConstraintProto::ConstraintCase::kBoolOr   // sat/cp_model_utils.h:636
```

```rust
fasthash64(&bytes_of_field, seed)   // bytes from field.to_ne_bytes()
ConstraintCase::BoolOr
```

## 32. Templates over plain structs, and spans that hand back a sub-span

A C++ template over struct types that share member names (`RectangleT::x_min`) is a private trait with one accessor for each member the template reads, and an associated type for the member type. A function that returns an `absl::Span<int>` taken from its `absl::Span<int>` argument returns a `&'a mut [i32]` with the lifetime of that argument.

```cpp
template <typename RectangleT>
std::optional<std::pair<int, int>> FindOneIntersectionIfPresentImpl(absl::Span<const RectangleT> rectangles, ...);   // sat/diffn_util.cc:2287
absl::Span<int> FilterBoxesThatAreTooLarge(absl::Span<const Rectangle> cached_rectangles, absl::Span<const IntegerValue> energies, absl::Span<int> boxes);   // sat/diffn_util.h:199
```

```rust
fn find_one_intersection_if_present_impl<R: RectangleCoordinates>(rectangles: &[R], ...) -> Option<(i32, i32)>
pub fn filter_boxes_that_are_too_large<'a>(cached_rectangles: &[Rectangle], energies: &[IntegerValue], boxes: &'a mut [i32]) -> &'a mut [i32]
```

## 33. Messages that print a double

`absl::StrCat` prints a double with six significant digits, as `printf("%g")` does, so `10000000000000000000000000` prints as `1e+25`. The Rust `{}` prints the shortest text that reads back to the same double, which differs. A file that prints a double in a message calls a private `six_digits_to_string`.

```cpp
return absl::StrCat("... [value = ", max_valid_magnitude, "] in objective: ", ...);   // sat/cp_model_checker.cc:991
```

```rust
format!("... [value = {}] in objective: {}", six_digits_to_string(max_valid_magnitude), ...)
```

## 34. Macros that return early

A C++ macro that returns from the calling function is a `macro_rules!` macro. Rust reads macros in file order, so the definition comes before the first use.

```cpp
#define RETURN_IF_NOT_EMPTY(statement)  // sat/cp_model_checker.cc:60
  do { const std::string error_message = statement; if (!error_message.empty()) return error_message; } while (false)
```

```rust
macro_rules! return_if_not_empty { ($statement:expr) => {{ let error_message: String = $statement; if !error_message.is_empty() { return error_message; } }}; }
```

## 35. Private functions that read one case of a `oneof`

A private C++ function that is only called after a `switch` on the constraint kind, and that reads the payload again through the getter of its kind, takes the payload in Rust. The `switch` passes it. A function that cannot tell the kind from its caller, such as one that reads a referenced constraint, matches the `oneof` itself and falls back to the empty default message as the C++ getter does (rule 26).

```cpp
bool BoolOrConstraintIsFeasible(const ConstraintProto& ct) { for (const int lit : ct.bool_or().literals()) ...   // sat/cp_model_checker.cc:1412
```

```rust
fn bool_or_constraint_is_feasible(&self, arg: &BoolArgumentProto) -> bool
```

## 36. Signed overflow that only an invalid input reaches

Where upstream signed arithmetic overflows only for an input that the validation of the same file rejects, the port keeps the plain operator, which panics in every profile because overflow checks are on, and the comment of the public function says so. This replaces the wrapping of rule 3 for that case, since the value an optimised C++ build gives there is not defined and is not a result to rely on. A division by zero, which stops the C++ process, panics in the same way.

```cpp
sum += variable_values_[vars[i]] * coeffs[i];   // sat/cp_model_checker.cc:1458
```

```rust
sum += self.variable_values[vars[i] as usize] * coeffs[i];
```
