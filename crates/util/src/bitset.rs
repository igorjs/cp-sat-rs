// Copyright 2010-2025 Google LLC
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
//
// Ported from ortools/util/bitset.h at commit c931f5f7. Changed: translated from C++ to Rust.
// Ported from ortools/util/bitset.cc at commit c931f5f7. Changed: translated from C++ to Rust.

//! Various utility functions on bitsets.

use core::cell::Cell;
use core::fmt::Debug;
use core::marker::PhantomData;
use core::ops::Index;

use crate::saturated_arithmetic::StrongInt64;

// Basic bit operations

/// Useful constants: word and double word will all bits set.
pub const K_ALL_BITS64: u64 = 0xFFFFFFFFFFFFFFFF;
pub const K_ALL_BITS_BUT_LSB64: u64 = 0xFFFFFFFFFFFFFFFE;

/// Returns a word with only bit pos set.
pub fn one_bit64(pos: i32) -> u64 {
    1u64 << pos
}
pub fn one_bit32(pos: i32) -> u32 {
    1u32 << pos
}

/// Returns the word with only the least significant bit of n set.
pub fn least_significant_bit_word64(n: u64) -> u64 {
    n & !n.wrapping_sub(1)
}

pub mod internal {
    // Performance comparison of LSB/MSB implementations (BM_BitIndex on x86-64):
    //
    // Operation | Implementation      | Scalar (kNo)  | Vectorizable loop (kYes)
    // ----------+---------------------+---------------+-------------------------
    // LSB 32    | Basic (bsearch)     | ~238M items/s | ~438M items/s
    //           | DeBruijn (lookup)   | ~1.07G items/s| ~3.15G items/s
    //           | Std (trailing_zeros)| ~1.08G items/s| ~1.59G items/s
    // ----------+---------------------+---------------+-------------------------
    // LSB 64    | Basic (bsearch)     | ~201M items/s | ~365M items/s
    //           | DeBruijn (lookup)   | ~1.08G items/s| ~3.18G items/s
    //           | Std (trailing_zeros)| ~1.08G items/s| ~1.07G items/s
    // ----------+---------------------+---------------+-------------------------
    // MSB 32    | Basic (bsearch)     | ~247M items/s | ~462M items/s
    //           | Std (leading_zeros) | ~643M items/s | ~800M items/s
    // ----------+---------------------+---------------+-------------------------
    // MSB 64    | Basic (bsearch)     | ~207M items/s | ~356M items/s
    //           | Std (leading_zeros) | ~427M items/s | ~639M items/s

    // Implementation notes:
    // Alternative algorithms are retained here to allow performance evaluation in
    // benchmarks across platforms, though the default implementation (Std) is
    // usually the best choice.
    // - Std: Portable and compiles down to hardware bit-scan instructions (tzcnt/bsf and
    //   lzcnt/bsr). Guarding n == 0 in most_significant_bit_position matches the contract
    //   of Basic and allows the compiler to fold `31 - (31 ^ bsr(n))` into `bsr(n)` on
    //   baseline x86-64 targets without LZCNT.
    // - DeBruijn (De Bruijn multiplication + 64/32-entry table lookup): Matches hardware
    //   instruction latency in scalar dependency chains and achieves ~2x higher throughput
    //   in tight independent loops because the multiply-shift-load sequence pipelines
    //   across multiple execution ports, at the cost of a static lookup table in L1 cache.
    // - Basic (binary search / shift-and-mask fallback): ~3-5x slower than hardware
    //   bit-scan or De Bruijn implementations due to sequential conditional masks and
    //   shifts.
    #[derive(Clone, Copy, Debug, PartialEq, Eq)]
    pub enum BitScanAlgorithm {
        Std,
        DeBruijn,
        Basic,
    }

    /// The unsigned types, `u32` and `u64`, that the two scans accept.
    pub trait BitScanWord: Copy {
        fn least_significant_bit_position(self, algo: BitScanAlgorithm) -> i32;
        fn most_significant_bit_position(self, algo: BitScanAlgorithm) -> i32;
    }

    // The steps of the `Basic` least significant bit scan that `u32` and `u64` share.
    macro_rules! least_significant_bit_basic_steps {
        ($n:ident, $pos:ident) => {
            if $n & 0x0000FFFF != 0 {
                $pos -= 16;
            } else {
                $n >>= 16;
            }
            if $n & 0x000000FF != 0 {
                $pos -= 8;
            } else {
                $n >>= 8;
            }
            if $n & 0x0000000F != 0 {
                $pos -= 4;
            } else {
                $n >>= 4;
            }
            if $n & 0x00000003 != 0 {
                $pos -= 2;
            } else {
                $n >>= 2;
            }
            if $n & 0x00000001 != 0 {
                $pos -= 1;
            }
        };
    }

    // The steps of the `Basic` most significant bit scan that `u32` and `u64` share.
    macro_rules! most_significant_bit_basic_steps {
        ($n:ident, $b:ident, $all_bits:ident) => {
            if 0 != ($n & ($all_bits << (1 << 4))) {
                $b |= 1 << 4;
                $n >>= 1 << 4;
            }
            if 0 != ($n & ($all_bits << (1 << 3))) {
                $b |= 1 << 3;
                $n >>= 1 << 3;
            }
            if 0 != ($n & ($all_bits << (1 << 2))) {
                $b |= 1 << 2;
                $n >>= 1 << 2;
            }
            if 0 != ($n & ($all_bits << (1 << 1))) {
                $b |= 1 << 1;
                $n >>= 1 << 1;
            }
            if 0 != ($n & ($all_bits << (1 << 0))) {
                $b |= 1 << 0;
            }
        };
    }

    // `DeBruijn` has no most significant bit scan: upstream rejects it with a `static_assert`.
    const NO_DE_BRUIJN_MSB: &str = "DeBruijn is not implemented for MostSignificantBitPosition";

    impl BitScanWord for u64 {
        fn least_significant_bit_position(self, algo: BitScanAlgorithm) -> i32 {
            let mut n = self;
            match algo {
                BitScanAlgorithm::Std => n.trailing_zeros() as i32,
                BitScanAlgorithm::DeBruijn => {
                    const K_SEQ: u64 = 0x0218a392dd5fb34f;
                    #[rustfmt::skip]
                    const K_TAB: [i32; 64] = [
                        // initialized by 'kTab[(kSeq << i) >> 58] = i
                        0,  1,  2,  7,  3,  13, 8,  19, 4,  25, 14, 28, 9,  52, 20, 58,
                        5,  17, 26, 56, 15, 38, 29, 40, 10, 49, 53, 31, 21, 34, 59, 42,
                        63, 6,  12, 18, 24, 27, 51, 57, 16, 55, 37, 39, 48, 30, 33, 41,
                        62, 11, 23, 50, 54, 36, 47, 32, 61, 22, 35, 46, 60, 45, 44, 43,
                    ];
                    K_TAB[((n & (!n).wrapping_add(1)).wrapping_mul(K_SEQ) >> 58) as usize]
                }
                BitScanAlgorithm::Basic => {
                    debug_assert_ne!(n, 0);
                    let mut pos = (u64::BITS - 1) as i32;
                    if n & 0x00000000FFFFFFFF != 0 {
                        pos -= 32;
                    } else {
                        n >>= 32;
                    }
                    least_significant_bit_basic_steps!(n, pos);
                    pos
                }
            }
        }

        fn most_significant_bit_position(self, algo: BitScanAlgorithm) -> i32 {
            let mut n = self;
            match algo {
                BitScanAlgorithm::Std => {
                    const K_MAX_BIT: i32 = (u64::BITS - 1) as i32;
                    if n == 0 {
                        0
                    } else {
                        K_MAX_BIT - n.leading_zeros() as i32
                    }
                }
                BitScanAlgorithm::DeBruijn => panic!("{NO_DE_BRUIJN_MSB}"),
                BitScanAlgorithm::Basic => {
                    const K_ALL_BITS: u64 = !0;
                    let mut b = 0;
                    if 0 != (n & (K_ALL_BITS << (1 << 5))) {
                        b |= 1 << 5;
                        n >>= 1 << 5;
                    }
                    most_significant_bit_basic_steps!(n, b, K_ALL_BITS);
                    b
                }
            }
        }
    }

    impl BitScanWord for u32 {
        fn least_significant_bit_position(self, algo: BitScanAlgorithm) -> i32 {
            let mut n = self;
            match algo {
                BitScanAlgorithm::Std => n.trailing_zeros() as i32,
                BitScanAlgorithm::DeBruijn => {
                    const K_SEQ: u32 = 0x077CB531; // de Bruijn sequence
                    #[rustfmt::skip]
                    const K_TAB: [i32; 32] = [
                        // initialized by 'kTab[(kSeq << i) >> 27] = i
                        0,  1,  28, 2,  29, 14, 24, 3, 30, 22, 20, 15, 25, 17, 4,  8,
                        31, 27, 13, 23, 21, 19, 16, 7, 26, 12, 18, 6,  11, 5,  10, 9,
                    ];
                    K_TAB[((n & (!n).wrapping_add(1)).wrapping_mul(K_SEQ) >> 27) as usize]
                }
                BitScanAlgorithm::Basic => {
                    debug_assert_ne!(n, 0);
                    let mut pos = (u32::BITS - 1) as i32;
                    least_significant_bit_basic_steps!(n, pos);
                    pos
                }
            }
        }

        fn most_significant_bit_position(self, algo: BitScanAlgorithm) -> i32 {
            let mut n = self;
            match algo {
                BitScanAlgorithm::Std => {
                    const K_MAX_BIT: i32 = (u32::BITS - 1) as i32;
                    if n == 0 {
                        0
                    } else {
                        K_MAX_BIT - n.leading_zeros() as i32
                    }
                }
                BitScanAlgorithm::DeBruijn => panic!("{NO_DE_BRUIJN_MSB}"),
                BitScanAlgorithm::Basic => {
                    const K_ALL_BITS: u32 = !0;
                    let mut b = 0;
                    most_significant_bit_basic_steps!(n, b, K_ALL_BITS);
                    b
                }
            }
        }
    }

    // Upstream selects the algorithm with a template argument. Rust has no enum const
    // generics on stable, so the algorithm is an argument.
    pub fn least_significant_bit_position<T: BitScanWord>(algo: BitScanAlgorithm, n: T) -> i32 {
        n.least_significant_bit_position(algo)
    }

    pub fn most_significant_bit_position<T: BitScanWord>(algo: BitScanAlgorithm, n: T) -> i32 {
        n.most_significant_bit_position(algo)
    }
}

/// Returns the least significant bit position in n.
pub fn least_significant_bit_position64(n: u64) -> i32 {
    debug_assert_ne!(n, 0);
    internal::least_significant_bit_position(internal::BitScanAlgorithm::Std, n)
}

pub fn least_significant_bit_position32(n: u32) -> i32 {
    debug_assert_ne!(n, 0);
    internal::least_significant_bit_position(internal::BitScanAlgorithm::Std, n)
}

/// Returns the most significant bit position in n.
pub fn most_significant_bit_position64(n: u64) -> i32 {
    internal::most_significant_bit_position(internal::BitScanAlgorithm::Std, n)
}

pub fn most_significant_bit_position32(n: u32) -> i32 {
    internal::most_significant_bit_position(internal::BitScanAlgorithm::Std, n)
}

// ----- Bitset operators -----
// Bitset: array of uint64_t words

// Bit operators used to manipulates bitsets.

/// Returns the bit number in the word computed by `bit_offset64`, corresponding to the bit
/// at position pos in the bitset.
// TODO(user): rename BitPos and BitOffset to something more understandable.
pub fn bit_pos64(pos: u64) -> u32 {
    (pos & 63) as u32
}

/// Returns the word number corresponding to bit number pos.
pub fn bit_offset64(pos: u64) -> u64 {
    pos >> 6
}

/// Returns the number of words needed to store size bits.
pub fn bit_length64(size: u64) -> u64 {
    size.wrapping_add(63) >> 6
}

/// Returns the bit number in the bitset of the first bit of word number v.
pub fn bit_shift64(v: u64) -> u64 {
    v << 6
}

/// Returns true if the bit pos is set in bitset.
pub fn is_bit_set64(bitset: &[u64], pos: u64) -> bool {
    bitset[bit_offset64(pos) as usize] & one_bit64(bit_pos64(pos) as i32) != 0
}

/// Sets the bit pos to true in bitset.
pub fn set_bit64(bitset: &mut [u64], pos: u64) {
    bitset[bit_offset64(pos) as usize] |= one_bit64(bit_pos64(pos) as i32);
}

/// Sets the bit pos to false in bitset.
pub fn clear_bit64(bitset: &mut [u64], pos: u64) {
    bitset[bit_offset64(pos) as usize] &= !one_bit64(bit_pos64(pos) as i32);
}

// Clears bits strictly below `start % 64` in `bucket` (keeps [start % 64, 63]).
fn clear_bits_before(bucket: u64, start: u64) -> u64 {
    let shift = bit_pos64(start);
    (bucket >> shift) << shift
}

// Clears bits strictly above `end % 64` in `bucket` (keeps [0, end % 64]).
fn clear_bits_after(bucket: u64, end: u64) -> u64 {
    let shift = 63 - bit_pos64(end);
    (bucket << shift) >> shift
}

/// Returns the number of bits set in bitset between positions start and end.
pub fn bit_count_range64(bits: &[u64], start: u64, end: u64) -> u64 {
    debug_assert!(start <= end);
    let mut offset = bit_offset64(start);
    let offset_end = bit_offset64(end);
    let mut bucket = clear_bits_before(bits[offset as usize], start);
    let mut bit_count: u64 = 0;
    while offset < offset_end {
        bit_count += bucket.count_ones() as u64;
        offset += 1;
        bucket = bits[offset as usize];
    }
    bucket = clear_bits_after(bucket, end);
    bit_count + bucket.count_ones() as u64
}

/// Returns true if no bits are set in bitset between start and end.
pub fn is_empty_range64(bits: &[u64], start: u64, end: u64) -> bool {
    least_significant_bit_position64_in_range(bits, start, end) == -1
}

/// Returns the first bit set in bitset between start and max_bit.
pub fn least_significant_bit_position64_in_range(bits: &[u64], start: u64, end: u64) -> i64 {
    debug_assert!(start <= end);
    let mut offset = bit_offset64(start);
    let offset_end = bit_offset64(end);
    let mut bucket = clear_bits_before(bits[offset as usize], start);
    while bucket == 0 && offset < offset_end {
        offset += 1;
        bucket = bits[offset as usize];
    }
    if offset == offset_end {
        bucket = clear_bits_after(bucket, end);
    }
    if bucket == 0 {
        return -1;
    }
    (bit_shift64(offset) + least_significant_bit_position64(bucket) as u64) as i64
}

/// Returns the last bit set in bitset between min_bit and start.
pub fn most_significant_bit_position64_in_range(bits: &[u64], start: u64, end: u64) -> i64 {
    debug_assert!(start <= end);
    let offset_start = bit_offset64(start);
    let mut offset = bit_offset64(end);
    let mut bucket = clear_bits_after(bits[offset as usize], end);
    while bucket == 0 && offset > offset_start {
        offset -= 1;
        bucket = bits[offset as usize];
    }
    if offset == offset_start {
        bucket = clear_bits_before(bucket, start);
    }
    if bucket == 0 {
        return -1;
    }
    (bit_shift64(offset) + most_significant_bit_position64(bucket) as u64) as i64
}

/// Unsafe versions of the functions above where respectively end and start are supposed to
/// be set.
pub fn unsafe_least_significant_bit_position64(bits: &[u64], start: u64, end: u64) -> i64 {
    debug_assert!(start <= end);
    debug_assert!(is_bit_set64(bits, end));
    let mut offset = bit_offset64(start);
    let mut bucket = clear_bits_before(bits[offset as usize], start);
    while bucket == 0 {
        offset += 1;
        bucket = bits[offset as usize];
    }
    (bit_shift64(offset) + least_significant_bit_position64(bucket) as u64) as i64
}

pub fn unsafe_most_significant_bit_position64(bits: &[u64], start: u64, end: u64) -> i64 {
    debug_assert!(start <= end);
    debug_assert!(is_bit_set64(bits, start));
    let mut offset = bit_offset64(end);
    let mut bucket = clear_bits_after(bits[offset as usize], end);
    while bucket == 0 {
        offset -= 1;
        bucket = bits[offset as usize];
    }
    (bit_shift64(offset) + most_significant_bit_position64(bucket) as u64) as i64
}

/// An integer or a strong integer type that indexes a [`Bitset64`].
///
/// `value_as_u64` is `Value(index)` of upstream converted to `uint64_t`, so a negative index
/// is sign extended. `from_u64` is the explicit conversion `IndexType(n)`.
pub trait BitsetIndex: Copy + PartialOrd + Debug {
    fn value_as_u64(self) -> u64;
    fn from_u64(value: u64) -> Self;
}

macro_rules! impl_bitset_index_for_integer {
    ($($int:ty),+) => {
        $(
            impl BitsetIndex for $int {
                fn value_as_u64(self) -> u64 {
                    self as i64 as u64
                }
                fn from_u64(value: u64) -> Self {
                    value as $int
                }
            }
        )+
    };
}

impl_bitset_index_for_integer!(i32, i64, usize);

impl<T: StrongInt64 + PartialOrd + Debug> BitsetIndex for T {
    fn value_as_u64(self) -> u64 {
        self.value() as u64
    }
    fn from_u64(value: u64) -> Self {
        T::from_value(value as i64)
    }
}

// Stores `size` and checks `0 <= index < size` in debug builds; empty in release builds so
// a view stays as small as its bucket slice.
#[cfg(debug_assertions)]
#[derive(Clone, Copy)]
struct BoundsChecker<I: BitsetIndex> {
    size: I,
}

#[cfg(debug_assertions)]
impl<I: BitsetIndex> BoundsChecker<I> {
    fn new(size: I) -> Self {
        BoundsChecker { size }
    }

    fn check(&self, index: I) {
        debug_assert!(index >= I::from_u64(0));
        debug_assert!(index < self.size);
    }
}

#[cfg(not(debug_assertions))]
#[derive(Clone, Copy)]
struct BoundsChecker<I: BitsetIndex>(PhantomData<I>);

#[cfg(not(debug_assertions))]
impl<I: BitsetIndex> BoundsChecker<I> {
    fn new(_size: I) -> Self {
        BoundsChecker(PhantomData)
    }

    fn check(&self, _index: I) {}
}

/// Marks a view that can only read.
#[derive(Clone, Copy, Debug)]
pub struct ReadOnly;

/// Marks a view that can read and write.
#[derive(Clone, Copy, Debug)]
pub struct ReadWrite;

/// Lightweight view caching the underlying bucket slice for tight loops. Performs bounds
/// checking in debug builds. The buckets are cells, so a view and other views of the same
/// bitset can live together, as the pointers of the C++ views do.
pub struct ViewImpl<'a, I: BitsetIndex, Access> {
    buckets: &'a [Cell<u64>],
    bounds_checker: BoundsChecker<I>,
    access: PhantomData<Access>,
}

pub type ConstView<'a, I> = ViewImpl<'a, I, ReadOnly>;
pub type View<'a, I> = ViewImpl<'a, I, ReadWrite>;

impl<I: BitsetIndex, Access> Clone for ViewImpl<'_, I, Access> {
    fn clone(&self) -> Self {
        *self
    }
}

impl<I: BitsetIndex, Access> Copy for ViewImpl<'_, I, Access> {}

impl<'a, I: BitsetIndex, Access> ViewImpl<'a, I, Access> {
    fn new(buckets: &'a [Cell<u64>], size: I) -> Self {
        ViewImpl {
            buckets,
            bounds_checker: BoundsChecker::new(size),
            access: PhantomData,
        }
    }

    /// Returns whether the bit at position `index` is set.
    pub fn is_set(&self, index: I) -> bool {
        self.get_bucket(index).get() & Self::get_bit_mask(index) != 0
    }

    fn get_bit_mask(index: I) -> u64 {
        one_bit64(bit_pos64(index.value_as_u64()) as i32)
    }

    fn get_bucket(&self, index: I) -> &'a Cell<u64> {
        self.bounds_checker.check(index);
        &self.buckets[bit_offset64(index.value_as_u64()) as usize]
    }
}

impl<I: BitsetIndex> ViewImpl<'_, I, ReadWrite> {
    pub fn clear(&mut self, index: I) {
        let bucket = self.get_bucket(index);
        bucket.set(bucket.get() & !Self::get_bit_mask(index));
    }

    pub fn set(&mut self, index: I) {
        let bucket = self.get_bucket(index);
        bucket.set(bucket.get() | Self::get_bit_mask(index));
    }
}

// `operator[]`: the bit as a `bool`.
impl<I: BitsetIndex, Access> Index<I> for ViewImpl<'_, I, Access> {
    type Output = bool;

    fn index(&self, index: I) -> &bool {
        if self.is_set(index) { &true } else { &false }
    }
}

/// A dynamically sized bitset indexed by `I` (an integral or strong index type) and
/// providing an efficient way to iterate over the true positions. It is tuned for maximum
/// speed, bound checking is performed though `debug_assert!` and is disabled in optimized
/// build.
#[derive(Clone)]
pub struct Bitset64<I: BitsetIndex = i64> {
    size: I,
    buckets: Vec<Cell<u64>>,
}

impl<I: BitsetIndex> Default for Bitset64<I> {
    /// Creates an empty bitset.
    fn default() -> Self {
        Bitset64 {
            size: I::from_u64(0),
            buckets: Vec::new(),
        }
    }
}

impl<I: BitsetIndex> Bitset64<I> {
    /// Constructs a bitset of `size` bits, all initialized to `0`.
    pub fn new(size: I) -> Self {
        Bitset64 {
            size,
            buckets: vec![Cell::new(0); Self::get_num_buckets(size)],
        }
    }

    /// Returns a read-only or mutable view caching the underlying bucket slice.
    pub fn const_view(&self) -> ConstView<'_, I> {
        ConstView::new(&self.buckets, self.size)
    }

    // Upstream `view()` is not `const` but a view and a const view live together in its
    // tests, so the mutable view borrows the bitset as shared, through the cells.
    pub fn view(&self) -> View<'_, I> {
        View::new(&self.buckets, self.size)
    }

    /// Returns the number of bits in this bitset.
    pub fn size(&self) -> I {
        self.size
    }

    /// Resizes the Bitset64 to the given number of bits. New bits are sets to `0`.
    pub fn resize_i32(&mut self, size: i32) {
        self.resize(I::from_u64(size as i64 as u64));
    }

    pub fn resize(&mut self, size: I) {
        self.buckets
            .resize(Self::get_num_buckets(size), Cell::new(0));
        let bit_pos = bit_pos64(size.value_as_u64());
        if size < self.size && bit_pos != 0 {
            let last = self
                .buckets
                .last()
                .expect("a bitset that shrinks to a partial bucket has a bucket");
            last.set(last.get() & (one_bit64(bit_pos as i32) - 1));
        }
        self.size = size;
    }

    /// Resizes the bitset to `size` bits and clears all the bits.
    pub fn clear_and_resize(&mut self, size: I) {
        self.size = size;
        self.buckets
            .resize(Self::get_num_buckets(size), Cell::new(0));
        self.clear_all();
    }

    /// Clears all the bits.
    pub fn clear_all(&mut self) {
        self.buckets.fill(Cell::new(0));
    }

    /// Clears the bit at position `index`.
    pub fn clear(&mut self, index: I) {
        self.view().clear(index);
    }

    /// Clears all the bits in the bucket containing bit `index`.
    pub fn clear_bucket(&mut self, index: I) {
        self.view().get_bucket(index).set(0);
    }

    /// Returns whether the bit at position `index` is set.
    pub fn is_set(&self, index: I) -> bool {
        self.const_view().is_set(index)
    }

    /// Sets the bit at position `index`.
    pub fn set(&mut self, index: I) {
        self.view().set(index);
    }

    /// Set the bit at position `index` to 1 when value is `true`, and to 0 otherwise.
    pub fn set_with_value(&mut self, index: I, value: bool) {
        if value {
            self.view().set(index);
        } else {
            self.view().clear(index);
        }
    }

    /// Copies the bucket containing bit `index` from `other` to `*this`.
    pub fn copy_bucket(&mut self, other: &Bitset64<I>, index: I) {
        let value = other.const_view().get_bucket(index).get();
        self.view().get_bucket(index).set(value);
    }

    /// Replaces `*this` with the bitwise intersection of `*this` and `other`. The bitsets
    /// do not have to be the same size; if `other` is smaller, higher-order bits are
    /// cleared to 0.
    pub fn intersection(&mut self, other: &Bitset64<I>) {
        let common = self.buckets.len().min(other.buckets.len());
        for (bucket, other_bucket) in self.buckets.iter().zip(&other.buckets) {
            bucket.set(bucket.get() & other_bucket.get());
        }
        self.buckets[common..].fill(Cell::new(0));
    }

    /// Sets `*this` to the bitwise intersection of `a` and `b`, which must have the same
    /// `size()`.
    pub fn set_to_intersection_of(&mut self, a: &Bitset64<I>, b: &Bitset64<I>) {
        debug_assert_eq!(a.size(), b.size());
        self.resize(a.size());
        for ((bucket, a_bucket), b_bucket) in self.buckets.iter().zip(&a.buckets).zip(&b.buckets) {
            bucket.set(a_bucket.get() & b_bucket.get());
        }
    }

    /// Replaces `*this` with the bitwise union of `*this` and `other`. The bitsets do not
    /// have to be the same size; if `other` is smaller, higher-order bits of `*this` are
    /// unchanged, and if `other` is larger, its extra bits are ignored.
    pub fn union(&mut self, other: &Bitset64<I>) {
        for (bucket, other_bucket) in self.buckets.iter().zip(&other.buckets) {
            bucket.set(bucket.get() | other_bucket.get());
        }
    }

    /// Returns true if all bits are 0.
    pub fn is_all_false(&self) -> bool {
        self.buckets.iter().all(|bucket| bucket.get() == 0)
    }

    /// Returns iterators over the indices of bits set to 1, as `begin()` and `end()` do
    /// upstream. A `for` loop over `&bitset` visits the same indices.
    pub fn begin(&self) -> Bitset64Iterator<'_, I> {
        Bitset64Iterator::new(self)
    }

    pub fn end(&self) -> Bitset64Iterator<'_, I> {
        Bitset64Iterator::at_bucket(self.buckets.len())
    }

    fn get_num_buckets(size: I) -> usize {
        assert!(size >= I::from_u64(0));
        bit_length64(size.value_as_u64()) as usize
    }
}

impl<I: BitsetIndex> Index<I> for Bitset64<I> {
    type Output = bool;

    fn index(&self, index: I) -> &bool {
        if self.is_set(index) { &true } else { &false }
    }
}

impl<'a, I: BitsetIndex> IntoIterator for &'a Bitset64<I> {
    type Item = I;
    type IntoIter = Bitset64Iterator<'a, I>;

    fn into_iter(self) -> Bitset64Iterator<'a, I> {
        self.begin()
    }
}

/// Forward iterator over the indices of set bits.
///
/// IMPORTANT: Because `Bitset64Iterator` caches the current 64-bit bucket, modifying the
/// bitset while iterating produces undefined results.
#[derive(Clone, Debug)]
pub struct Bitset64Iterator<'a, I: BitsetIndex> {
    buckets: &'a [Cell<u64>],
    size: usize,
    bucket: usize,
    current: u64,
    index_type: PhantomData<I>,
}

impl<I: BitsetIndex> Default for Bitset64Iterator<'_, I> {
    fn default() -> Self {
        Self::at_bucket(0)
    }
}

impl<'a, I: BitsetIndex> Bitset64Iterator<'a, I> {
    fn new(bitset: &'a Bitset64<I>) -> Self {
        let mut iterator = Bitset64Iterator {
            buckets: &bitset.buckets,
            size: bitset.buckets.len(),
            bucket: 0,
            current: 0,
            index_type: PhantomData,
        };
        iterator.advance_to_non_zero_bucket();
        iterator
    }

    fn at_bucket(bucket: usize) -> Self {
        Bitset64Iterator {
            buckets: &[],
            size: 0,
            bucket,
            current: 0,
            index_type: PhantomData,
        }
    }

    /// `operator*`: the index of the current set bit.
    pub fn current(&self) -> I {
        I::from_u64(
            bit_shift64(self.bucket as u64) | least_significant_bit_position64(self.current) as u64,
        )
    }

    /// `++it`.
    pub fn pre_increment(&mut self) -> &mut Self {
        self.current &= self.current.wrapping_sub(1); // Clears the LSB.
        if self.current == 0 {
            self.bucket += 1;
            self.advance_to_non_zero_bucket();
        }
        self
    }

    /// `it++`.
    pub fn post_increment(&mut self) -> Self {
        let other = self.clone();
        self.pre_increment();
        other
    }

    fn advance_to_non_zero_bucket(&mut self) {
        // `buckets` is never read when it is empty.
        debug_assert!(!self.buckets.is_empty() || self.bucket == self.size);
        while self.bucket != self.size {
            self.current = self.buckets[self.bucket].get();
            if self.current != 0 {
                break;
            }
            self.bucket += 1;
        }
    }
}

impl<I: BitsetIndex> PartialEq for Bitset64Iterator<'_, I> {
    fn eq(&self, other: &Self) -> bool {
        self.current == other.current && self.bucket == other.bucket
    }
}

impl<I: BitsetIndex> Iterator for Bitset64Iterator<'_, I> {
    type Item = I;

    fn next(&mut self) -> Option<I> {
        if self.bucket >= self.size {
            return None;
        }
        let index = self.current();
        self.pre_increment();
        Some(index)
    }
}

/// Specialized version of Bitset64 that allows to query the last bit set more efficiently.
///
/// It is neither `Clone` nor `Copy`.
#[derive(Debug)]
pub struct BitQueue64 {
    size: i32,
    top: i32,
    data: Vec<u64>,
}

impl Default for BitQueue64 {
    fn default() -> Self {
        BitQueue64 {
            size: 0,
            top: -1,
            data: Vec::new(),
        }
    }
}

impl BitQueue64 {
    pub fn new(size: i32) -> Self {
        BitQueue64 {
            size,
            top: -1,
            data: vec![0; bit_length64(size as u64) as usize],
        }
    }

    pub fn increase_size(&mut self, size: i32) {
        assert!(size >= self.size);
        self.size = size;
        self.data.resize(bit_length64(size as u64) as usize, 0);
    }

    pub fn clear_and_resize(&mut self, size: i32) {
        self.top = -1;
        self.size = size;
        self.data.clear();
        self.data.resize(bit_length64(size as u64) as usize, 0);
    }

    pub fn set(&mut self, i: i32) {
        debug_assert!(i >= 0);
        debug_assert!(i < self.size);
        self.top = self.top.max(i);
        self.data[bit_offset64(i as u64) as usize] |= one_bit64(bit_pos64(i as u64) as i32);
    }

    /// Sets all the bits from 0 up to i-1 to 1.
    pub fn set_all_before(&mut self, i: i32) {
        debug_assert!(i >= 0);
        debug_assert!(i < self.size);
        self.top = self.top.max(i - 1);
        let mut bucket_index = bit_offset64(i as u64) as i32;
        self.data[bucket_index as usize] |= one_bit64(bit_pos64(i as u64) as i32) - 1;
        bucket_index -= 1;
        while bucket_index >= 0 {
            self.data[bucket_index as usize] = K_ALL_BITS64;
            bucket_index -= 1;
        }
    }

    /// Returns the position of the highest bit set in O(1) or -1 if no bit is set.
    pub fn top(&self) -> i32 {
        self.top
    }

    /// Clears the `top()` bit and recomputes the position of the next `top()`.
    pub fn clear_top(&mut self) {
        debug_assert_ne!(self.top, -1);
        let mut bucket_index = bit_offset64(self.top as u64) as i32;
        self.data[bucket_index as usize] &= !one_bit64(bit_pos64(self.top as u64) as i32);
        let mut bucket = self.data[bucket_index as usize];
        while bucket == 0 {
            if bucket_index == 0 {
                self.top = -1;
                return;
            }
            bucket_index -= 1;
            bucket = self.data[bucket_index as usize];
        }

        // NOTE(user): I experimented with reversing the bit order in a bucket to use
        // LeastSignificantBitPosition64() and it is only slightly faster at the cost of a
        // lower Set() speed. So I preferred this version.
        self.top = (bit_shift64(bucket_index as u64)
            + most_significant_bit_position64(bucket) as u64) as i32;
    }
}

/// A simple utility class to set/unset integer in a range [0, size). This is optimized for
/// sparsity.
///
/// It is not `Clone`: `copy_from` copies it. Upstream `SetUnsafe` is not ported: it needs
/// a view of the bitset and `&mut self` at once, which the borrow checker refuses.
pub struct SparseBitset<I: BitsetIndex = i64> {
    bitset: Bitset64<I>,
    to_clear: Vec<I>,
}

impl<I: BitsetIndex> Default for SparseBitset<I> {
    fn default() -> Self {
        SparseBitset {
            bitset: Bitset64::default(),
            to_clear: Vec::new(),
        }
    }
}

impl<I: BitsetIndex> SparseBitset<I> {
    pub fn new(size: I) -> Self {
        SparseBitset {
            bitset: Bitset64::new(size),
            to_clear: Vec::new(),
        }
    }

    pub fn size(&self) -> I {
        self.bitset.size()
    }

    pub fn reset_all_to_false(&mut self) {
        self.clear_and_resize(self.size());
    }

    pub fn clear_and_resize(&mut self, size: I) {
        // As of 19/03/2014, experiments show that this is a reasonable threshold.
        const K_SPARSE_THRESHOLD: usize = 300;
        if ((self.to_clear.len() * K_SPARSE_THRESHOLD) as u64) < size.value_as_u64() {
            for &i in &self.to_clear {
                self.bitset.clear_bucket(i);
            }
            self.to_clear.clear();
            self.bitset.resize(size);
        } else {
            self.bitset.clear_and_resize(size);
            self.to_clear.clear();
        }
    }

    pub fn resize(&mut self, size: I) {
        if size < self.bitset.size() {
            let mut new_index = 0;
            for at in 0..self.to_clear.len() {
                let index = self.to_clear[at];
                if index < size {
                    self.to_clear[new_index] = index;
                    new_index += 1;
                }
            }
            self.to_clear.truncate(new_index);
        }
        self.bitset.resize(size);
    }

    pub fn set(&mut self, index: I) {
        if !self.bitset.is_set(index) {
            self.bitset.set(index);
            self.to_clear.push(index);
        }
    }

    pub fn copy_from(&mut self, other: &SparseBitset<I>) {
        self.bitset = other.bitset.clone();
        self.to_clear.clone_from(&other.to_clear);
    }

    pub fn view(&self) -> View<'_, I> {
        self.bitset.view()
    }

    pub fn const_view(&self) -> ConstView<'_, I> {
        self.bitset.const_view()
    }

    pub fn clear(&mut self, index: I) {
        self.bitset.clear(index);
    }

    pub fn number_of_set_calls_with_different_arguments(&self) -> i32 {
        self.to_clear.len() as i32
    }

    pub fn positions_set_at_least_once(&self) -> &[I] {
        &self.to_clear
    }

    /// Tells the class that all its bits are cleared, so it can reset `to_clear` to the
    /// empty vector. Note that this call is "unsafe" since the fact that the class is
    /// actually all cleared is only checked in debug mode.
    ///
    /// This is useful to iterate on the "set" positions while clearing them for instance.
    /// This way, after the loop, a client can call this for efficiency.
    pub fn notify_all_clear(&mut self) {
        #[cfg(debug_assertions)]
        for &index in &self.to_clear {
            assert!(!self.bitset.is_set(index));
        }
        self.to_clear.clear();
    }
}

impl<I: BitsetIndex> Index<I> for SparseBitset<I> {
    type Output = bool;

    fn index(&self, index: I) -> &bool {
        if self.bitset.is_set(index) {
            &true
        } else {
            &false
        }
    }
}

#[cfg(test)]
mod tests {
    use std::collections::HashSet;
    use std::ops::{BitOr, Shl};

    use cpsat_testutil::expect_death;
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::{RngCore, SeedableRng};

    use super::*;
    use crate::define_strong_int64_type;

    // A uniform draw in [lo, hi).
    fn uniform(random: &mut ChaCha8Rng, lo: i32, hi: i32) -> i32 {
        lo + (random.next_u64() % (hi - lo) as u64) as i32
    }

    // A draw in [lo, hi] that is uniform on the number of bits first, as `absl::LogUniform`.
    fn log_uniform(random: &mut ChaCha8Rng, lo: i32, hi: i32) -> i32 {
        let range = (hi - lo) as u32;
        let bits = 32 - range.leading_zeros();
        let limit = match uniform(random, 0, bits as i32 + 1) {
            0 => 0,
            e => ((1u64 << e) - 1).min(range as u64) as i32,
        };
        lo + uniform(random, 0, limit + 1)
    }

    // The values `start`, `start + 1`, ... below `end`, as the loop `for (TestIndex i(start); i < end; ++i)`.
    fn indices(start: i64, end: impl Into<i64>) -> impl Iterator<Item = TestIndex> {
        (start..end.into()).map(TestIndex::new)
    }

    // Runs one case of a typed test for each type of the list, as `TYPED_TEST` does. The
    // test names are written out because a macro cannot join identifiers.
    macro_rules! typed_test {
        ($body:ident, $($name:ident => $case:ty),+ $(,)?) => {
            $(
                #[test]
                fn $name() {
                    $body::<$case>();
                }
            )+
        };
    }

    ////////////////////////////////////////////////////////////////////////////////
    // Least / Most Significant Bit Position
    ////////////////////////////////////////////////////////////////////////////////

    mod bitset_index {
        use super::*;
        use internal::BitScanAlgorithm;

        // An enum to select between LSBIndex and MSBIndex.
        #[derive(Clone, Copy, PartialEq, Eq)]
        pub enum Function {
            Lsb,
            Msb,
        }

        // A type to describe a complete implementation.
        pub trait TestCase {
            type UIntT: Copy
                + BitOr<Output = Self::UIntT>
                + Shl<usize, Output = Self::UIntT>
                + From<u8>
                + internal::BitScanWord;
            const FUNCTION: Function;
            const ALGO: BitScanAlgorithm;
            fn compute(value: Self::UIntT) -> i32 {
                match Self::FUNCTION {
                    Function::Lsb => internal::least_significant_bit_position(Self::ALGO, value),
                    Function::Msb => internal::most_significant_bit_position(Self::ALGO, value),
                }
            }
        }

        macro_rules! test_case {
            ($name:ident, $function:expr, $uint:ty, $algo:expr) => {
                pub struct $name;
                impl TestCase for $name {
                    type UIntT = $uint;
                    const FUNCTION: Function = $function;
                    const ALGO: BitScanAlgorithm = $algo;
                }
            };
        }

        // Shorthand for all available types.
        test_case!(Lsb32Basic, Function::Lsb, u32, BitScanAlgorithm::Basic);
        test_case!(
            Lsb32DeBruijn,
            Function::Lsb,
            u32,
            BitScanAlgorithm::DeBruijn
        );
        test_case!(Lsb32Std, Function::Lsb, u32, BitScanAlgorithm::Std);
        test_case!(Lsb64Basic, Function::Lsb, u64, BitScanAlgorithm::Basic);
        test_case!(
            Lsb64DeBruijn,
            Function::Lsb,
            u64,
            BitScanAlgorithm::DeBruijn
        );
        test_case!(Lsb64Std, Function::Lsb, u64, BitScanAlgorithm::Std);
        test_case!(Msb32Basic, Function::Msb, u32, BitScanAlgorithm::Basic);
        test_case!(Msb32Std, Function::Msb, u32, BitScanAlgorithm::Std);
        test_case!(Msb64Basic, Function::Msb, u64, BitScanAlgorithm::Basic);
        test_case!(Msb64Std, Function::Msb, u64, BitScanAlgorithm::Std);

        fn bit_index_single_bit<T: TestCase>() {
            let type_bits = size_of::<T::UIntT>() * 8;
            for i in 0..type_bits {
                // Arrange
                let bit_at_i = T::UIntT::from(1) << i;
                // Act
                let position = T::compute(bit_at_i);
                // Assert
                assert_eq!(i as i32, position);
            }
            // NOTE: `least_significant_bit_position64` and `least_significant_bit_position32`
            // are currently unspecified for the 0 value so we don't test it.
            if T::FUNCTION == Function::Msb {
                assert_eq!(0, T::compute(T::UIntT::from(0)));
            }
        }

        fn bit_index_two_bits<T: TestCase>() {
            let type_bits = size_of::<T::UIntT>() * 8;
            for i in 0..type_bits {
                for j in 0..type_bits {
                    // Arrange
                    let bit_at_i = T::UIntT::from(1) << i;
                    let bit_at_j = T::UIntT::from(1) << j;
                    let combined = bit_at_i | bit_at_j;
                    let expected = if T::FUNCTION == Function::Lsb {
                        i.min(j)
                    } else {
                        i.max(j)
                    };
                    // Act
                    let position = T::compute(combined);
                    // Assert
                    assert_eq!(expected as i32, position);
                }
            }
        }

        typed_test!(
            bit_index_single_bit,
            bit_index_single_bit_lsb_32_basic => Lsb32Basic,
            bit_index_single_bit_lsb_32_de_bruijn => Lsb32DeBruijn,
            bit_index_single_bit_lsb_32_std => Lsb32Std,
            bit_index_single_bit_lsb_64_basic => Lsb64Basic,
            bit_index_single_bit_lsb_64_de_bruijn => Lsb64DeBruijn,
            bit_index_single_bit_lsb_64_std => Lsb64Std,
            bit_index_single_bit_msb_32_basic => Msb32Basic,
            bit_index_single_bit_msb_32_std => Msb32Std,
            bit_index_single_bit_msb_64_basic => Msb64Basic,
            bit_index_single_bit_msb_64_std => Msb64Std,
        );

        typed_test!(
            bit_index_two_bits,
            bit_index_two_bits_lsb_32_basic => Lsb32Basic,
            bit_index_two_bits_lsb_32_de_bruijn => Lsb32DeBruijn,
            bit_index_two_bits_lsb_32_std => Lsb32Std,
            bit_index_two_bits_lsb_64_basic => Lsb64Basic,
            bit_index_two_bits_lsb_64_de_bruijn => Lsb64DeBruijn,
            bit_index_two_bits_lsb_64_std => Lsb64Std,
            bit_index_two_bits_msb_32_basic => Msb32Basic,
            bit_index_two_bits_msb_32_std => Msb32Std,
            bit_index_two_bits_msb_64_basic => Msb64Basic,
            bit_index_two_bits_msb_64_std => Msb64Std,
        );
    }

    #[test]
    fn bitset_test_bit_pos64() {
        for i in 0..128 {
            // Arrange
            let expected = i % 64;
            // Act
            let position = bit_pos64(i as u64);
            // Assert
            assert_eq!(position, expected);
        }
    }

    #[test]
    fn bitset_test_bit_offset() {
        for i in 0..128u64 {
            // Arrange
            let expected = i / 64;
            // Act
            let offset = bit_offset64(i);
            // Assert
            assert_eq!(offset, expected);
        }
    }

    #[test]
    fn bitset_test_bit_length() {
        // Arrange
        // Act
        // Assert
        assert_eq!(0, bit_length64(0));
        assert_eq!(1, bit_length64(33));
        assert_eq!(1, bit_length64(64));
        assert_eq!(2, bit_length64(65));
    }

    #[test]
    fn bitset_test_one_bit() {
        // Arrange
        let k_bits32: u32 = 0;
        let k_bits64: u64 = 0;
        // Act
        let one32 = one_bit32(0);
        let one64 = one_bit64(0);
        // Assert
        assert_eq!(size_of_val(&k_bits32), size_of_val(&one32));
        assert_eq!(size_of_val(&k_bits64), size_of_val(&one64));
    }

    #[test]
    fn bitset_test_is_bit_set() {
        // Arrange
        let k_bits64: [u64; 2] = [0, 0xffffffff];
        // Act
        // Assert
        assert!(!is_bit_set64(&k_bits64, 0));
        assert!(is_bit_set64(&k_bits64, 64));
    }

    #[test]
    fn bitset_test_bit_shift() {
        // Arrange
        // Act
        // Assert
        assert_eq!(1 << 6, bit_shift64(1));
    }

    #[test]
    fn bitset_test_set_bit64() {
        const K_NUM_BUCKETS: usize = 10;
        const K_NUM_BITS: usize = K_NUM_BUCKETS * 64;
        let mut bits = [0u64; K_NUM_BUCKETS];
        for i in 0..K_NUM_BITS {
            // Arrange
            bits.fill(0);
            // Act
            set_bit64(&mut bits, i as u64);
            // Assert
            assert!(is_bit_set64(&bits, i as u64));
        }
    }

    #[test]
    fn bitset_test_clear_bit64() {
        const K_NUM_BUCKETS: usize = 10;
        const K_NUM_BITS: usize = K_NUM_BUCKETS * 64;
        let mut bits = [0u64; K_NUM_BUCKETS];
        for i in 0..K_NUM_BITS {
            // Arrange
            bits.fill(K_ALL_BITS64);
            // Act
            clear_bit64(&mut bits, i as u64);
            // Assert
            assert!(!is_bit_set64(&bits, i as u64));
        }
    }

    // Fills `bits` with random bits, and draws the start and the end of a random range.
    fn random_bits_and_range(
        randomizer: &mut ChaCha8Rng,
        i: i32,
        bits: &mut [u64],
        num_bits: i32,
    ) -> (i32, i32) {
        bits.fill(0);
        let num_to_set = log_uniform(randomizer, 0, num_bits);
        for _ in 0..num_to_set {
            set_bit64(bits, uniform(randomizer, 0, num_bits) as u64);
        }
        let mut s = uniform(randomizer, 0, num_bits);
        let mut e = if i % 2 == 0 {
            uniform(randomizer, 0, num_bits)
        } else {
            (num_bits - 1).min(s + uniform(randomizer, 0, 64))
        };
        if s > e {
            std::mem::swap(&mut s, &mut e);
        }
        (s, e)
    }

    #[test]
    fn bitset_test_bit_count_range64() {
        let mut randomizer = ChaCha8Rng::seed_from_u64(1);
        const K_TESTS: i32 = 1000;
        const K_NUM_BUCKETS: usize = 10;
        const K_NUM_BITS: i32 = K_NUM_BUCKETS as i32 * 64;
        let mut bits = [0u64; K_NUM_BUCKETS];
        for i in 0..K_TESTS {
            // Arrange
            let (s, e) = random_bits_and_range(&mut randomizer, i, &mut bits, K_NUM_BITS);
            let mut count = 0;
            for bit in s..=e {
                count += is_bit_set64(&bits, bit as u64) as u64;
            }
            // Act
            let counted = bit_count_range64(&bits, s as u64, e as u64);
            // Assert
            assert_eq!(count, counted);
        }
    }

    #[test]
    fn bitset_test_is_empty_range64() {
        let mut randomizer = ChaCha8Rng::seed_from_u64(1);
        const K_TESTS: i32 = 1000;
        const K_NUM_BUCKETS: usize = 10;
        const K_NUM_BITS: i32 = K_NUM_BUCKETS as i32 * 64;
        let mut bits = [0u64; K_NUM_BUCKETS];
        for i in 0..K_TESTS {
            // Arrange
            let (s, e) = random_bits_and_range(&mut randomizer, i, &mut bits, K_NUM_BITS);
            let mut empty = true;
            for bit in s..=e {
                if is_bit_set64(&bits, bit as u64) {
                    empty = false;
                    break;
                }
            }
            // Act
            let is_empty = is_empty_range64(&bits, s as u64, e as u64);
            // Assert
            assert_eq!(empty, is_empty);
        }
    }

    #[test]
    fn bitset_test_find_first_bit64() {
        let mut randomizer = ChaCha8Rng::seed_from_u64(1);
        const K_TESTS: i32 = 1000;
        const K_NUM_BUCKETS: usize = 10;
        const K_NUM_BITS: i32 = K_NUM_BUCKETS as i32 * 64;
        let mut bits = [0u64; K_NUM_BUCKETS];
        for i in 0..K_TESTS {
            // Arrange
            let (start, end) = random_bits_and_range(&mut randomizer, i, &mut bits, K_NUM_BITS);
            let mut bitup: i32 = -1;
            let mut bitdown: i32 = -1;
            for bit in start..=end {
                if is_bit_set64(&bits, bit as u64) {
                    bitdown = bit;
                    if bitup == -1 {
                        bitup = bit;
                    }
                }
            }
            let (start64, end64) = (start as u64, end as u64);
            // Act and Assert
            assert_eq!(
                bitup as i64,
                least_significant_bit_position64_in_range(&bits, start64, end64)
            );
            assert_eq!(
                bitdown as i64,
                most_significant_bit_position64_in_range(&bits, start64, end64)
            );
            if is_bit_set64(&bits, end64) {
                assert_eq!(
                    bitup as i64,
                    unsafe_least_significant_bit_position64(&bits, start64, end64)
                );
            }
            if is_bit_set64(&bits, start64) {
                assert_eq!(
                    bitdown as i64,
                    unsafe_most_significant_bit_position64(&bits, start64, end64)
                );
            }
            if bitup == -1 {
                set_bit64(&mut bits, end64);
                assert_eq!(
                    end as i64,
                    unsafe_least_significant_bit_position64(&bits, start64, end64)
                );
                clear_bit64(&mut bits, end64);

                set_bit64(&mut bits, start64);
                assert_eq!(
                    start as i64,
                    unsafe_most_significant_bit_position64(&bits, start64, end64)
                );
                clear_bit64(&mut bits, start64);
            }
        }
    }

    define_strong_int64_type!(TestIndex);

    #[test]
    fn sparse_bitset_test_empty() {
        // Arrange
        let bitset: SparseBitset<TestIndex> = SparseBitset::default();
        // Act
        let calls = bitset.number_of_set_calls_with_different_arguments();
        // Assert
        assert_eq!(0, calls);
    }

    #[test]
    fn sparse_bitset_test_basic_behavior() {
        // Arrange
        let mut bitset: SparseBitset<TestIndex> = SparseBitset::default();
        bitset.clear_and_resize(TestIndex::new(10));
        assert!(!bitset[TestIndex::new(1)]);
        assert!(!bitset[TestIndex::new(4)]);
        assert!(!bitset[TestIndex::new(6)]);
        // Act
        bitset.set(TestIndex::new(1));
        bitset.set(TestIndex::new(1));
        bitset.set(TestIndex::new(4));
        // Assert
        assert!(bitset[TestIndex::new(1)]);
        assert!(bitset[TestIndex::new(4)]);
        assert!(!bitset[TestIndex::new(6)]);
        assert_eq!(2, bitset.number_of_set_calls_with_different_arguments());
        bitset.clear(TestIndex::new(1));
        assert_eq!(2, bitset.number_of_set_calls_with_different_arguments());
        assert!(!bitset[TestIndex::new(1)]);
    }

    #[test]
    fn sparse_bitset_test_clear_and_resize() {
        const K_SIZE: i64 = 10000;
        let mut bitset: SparseBitset<i64> = SparseBitset::new(K_SIZE);
        for i in 0..K_SIZE {
            assert!(!bitset[i]);
        }
        bitset.set(10);
        bitset.set(100);
        bitset.set(9000);

        bitset.clear_and_resize(K_SIZE / 3);
        assert_eq!(bitset.size(), K_SIZE / 3);
        for i in 0..K_SIZE / 3 {
            assert!(!bitset[i]);
            bitset.set(i);
        }

        bitset.clear_and_resize(2 * K_SIZE);
        for i in 0..2 * K_SIZE {
            assert!(!bitset[i]);
        }
    }

    #[test]
    fn sparse_bitset_test_resize() {
        const K_SIZE: i64 = 10000;
        let mut bitset: SparseBitset<i64> = SparseBitset::new(K_SIZE);
        bitset.set(10);
        bitset.set(9000);
        bitset.set(100);

        bitset.resize(20000);
        assert_eq!(20000, bitset.size());
        assert_eq!(bitset.positions_set_at_least_once(), [10, 9000, 100]);

        bitset.resize(150);
        assert_eq!(150, bitset.size());
        assert_eq!(bitset.positions_set_at_least_once(), [10, 100]);

        bitset.resize(0);
        assert_eq!(0, bitset.size());
        assert!(bitset.positions_set_at_least_once().is_empty());
    }

    #[test]
    fn sparse_bitset_test_notify_all_clear() {
        const K_SIZE: i64 = 10000;
        let mut bitset: SparseBitset<i64> = SparseBitset::new(K_SIZE);
        bitset.set(10);
        bitset.set(9000);
        bitset.set(100);

        for index in bitset.positions_set_at_least_once().to_vec() {
            bitset.clear(index);
        }
        bitset.notify_all_clear();
        assert!(bitset.positions_set_at_least_once().is_empty());
    }

    #[test]
    fn sparse_bitset_reset_all_to_false() {
        const K_SIZE: i64 = 10000;
        let mut bitset: SparseBitset<i64> = SparseBitset::new(K_SIZE);
        bitset.set(10);
        bitset.set(100);
        bitset.set(9000);
        let mut bit_count = 0;
        for i in 0..K_SIZE {
            if bitset[i] {
                bit_count += 1;
            }
        }
        assert_eq!(3, bit_count);
        bitset.reset_all_to_false();
        for i in 0..K_SIZE {
            assert!(!bitset[i]);
        }
    }

    #[test]
    fn sparse_bitset_test_copy_from() {
        // Arrange
        let mut src: SparseBitset<TestIndex> = SparseBitset::new(TestIndex::new(100));
        src.set(TestIndex::new(7));
        src.set(TestIndex::new(70));

        let mut dst: SparseBitset<TestIndex> = SparseBitset::new(TestIndex::new(50));
        dst.set(TestIndex::new(3));
        // Act
        dst.copy_from(&src);
        // Assert
        assert_eq!(dst.size(), TestIndex::new(100));
        assert_eq!(
            dst.positions_set_at_least_once(),
            [TestIndex::new(7), TestIndex::new(70)]
        );
        for i in indices(0, 100) {
            assert_eq!(dst[i], i == TestIndex::new(7) || i == TestIndex::new(70));
        }
    }

    #[test]
    fn bitset64_test_basic_functions() {
        let k_size = TestIndex::new(100);
        let mut bitset: Bitset64<TestIndex> = Bitset64::new(k_size);

        assert_eq!(k_size, bitset.size());
        for i in indices(0, k_size) {
            assert!(!bitset.is_set(i));
        }

        for i in indices(0, k_size) {
            bitset.set(i);
            assert!(bitset.is_set(i));
            assert!(bitset.is_set(i));
            bitset.clear(i);
            assert!(!bitset.is_set(i));
            assert!(!bitset.is_set(i));
        }

        for i in indices(0, k_size) {
            bitset.set(i);
        }
        for i in indices(0, k_size) {
            assert!(bitset.is_set(i));
        }
        bitset.clear_and_resize(k_size);
        for i in indices(0, k_size) {
            assert!(!bitset.is_set(i));
        }
        for i in indices(0, k_size) {
            bitset.set(i);
        }
        bitset.clear_all();
        for i in indices(0, k_size) {
            assert!(!bitset.is_set(i));
        }
        for i in indices(0, k_size) {
            bitset.set(i);
        }
        assert_eq!((&bitset).into_iter().max().unwrap(), k_size - 1);
        bitset.clear_bucket(TestIndex::new(65));
        for i in indices(0, TestIndex::new(64)) {
            assert!(bitset.is_set(i));
        }
        for i in indices(64, k_size) {
            assert!(!bitset.is_set(i));
        }
    }

    #[test]
    fn bitset64_test_integer_specialization() {
        const K_SIZE: i32 = 1000;
        let mut bitset: Bitset64<i32> = Bitset64::new(K_SIZE);
        assert_eq!(K_SIZE, bitset.size());
        for i in 0..K_SIZE {
            assert!(!bitset.is_set(i));
        }
        for i in 0..K_SIZE {
            bitset.set(i);
            assert!(bitset.is_set(i));
            assert!(bitset.is_set(i));
            bitset.clear(i);
            assert!(!bitset.is_set(i));
            assert!(!bitset.is_set(i));
        }

        for i in 0..K_SIZE {
            bitset.set(i);
        }
        for i in 0..K_SIZE {
            assert!(bitset.is_set(i));
        }

        bitset.clear_and_resize(K_SIZE);
        for i in 0..K_SIZE {
            assert!(!bitset.is_set(i));
        }

        for i in 0..K_SIZE {
            bitset.set(i);
        }
        bitset.clear_and_resize(K_SIZE / 3);
        assert_eq!(bitset.size(), K_SIZE / 3);
        for i in 0..K_SIZE / 3 {
            assert!(!bitset.is_set(i));
            bitset.set(i);
        }

        bitset.clear_and_resize(3 * K_SIZE);
        for i in 0..3 * K_SIZE {
            assert!(!bitset.is_set(i));
        }
    }

    #[test]
    fn bitset64_test_resize() {
        let mut bitset: Bitset64<TestIndex> = Bitset64::default();
        assert_eq!(TestIndex::new(0), bitset.size());
        let k_large = TestIndex::new(100000);
        for i in indices(0, k_large) {
            bitset.resize(i + 1);
            bitset.set_with_value(i, i.value() % 2 != 0);
            assert_eq!(i + 1, bitset.size());
        }
        assert_eq!(k_large, bitset.size());
        for i in 2..10 {
            bitset.resize(k_large / i);
            bitset.resize(k_large);
            let mut copy_from_iterator: HashSet<TestIndex> = HashSet::new();
            for j in &bitset {
                copy_from_iterator.insert(j);
            }
            for j in indices(0, k_large) {
                let expected = (j.value() % 2 == 1) && j < k_large / i;
                assert_eq!(expected, bitset.is_set(j));
                assert_eq!(copy_from_iterator.contains(&j), bitset.is_set(j));
            }
        }
    }

    // The two comparison operators are each tested on their own, as upstream does.
    #[allow(clippy::eq_op, clippy::nonminimal_bool)]
    #[test]
    fn bitset64_test_iterator() {
        fn assert_forward_iterator<T: Iterator + Clone + PartialEq>() {}
        assert_forward_iterator::<Bitset64Iterator<'_, i32>>();
        let mut bitset: Bitset64<i32> = Bitset64::new(5);
        bitset.set(0);
        bitset.set(2);
        bitset.set(4);
        let mut it = bitset.begin();
        let mut next = it.clone();
        next.pre_increment();
        assert!(it != next);
        assert!(!(it == next));
        assert!(it == it);
        assert!(!(it != it));
        it.pre_increment();
        assert!(it == next);
        it.pre_increment();
        next.post_increment();
        assert!(it == next);
        next.pre_increment();
        assert_eq!(next, bitset.end());
        assert_eq!(bitset.end(), next);
        assert_ne!(it, next);
        assert_ne!(next, it);
    }

    #[test]
    fn bitset64_test_iterator_single_bit_zero() {
        let mut bitset: Bitset64<i32> = Bitset64::new(5);
        bitset.set(0);
        let begin = bitset.begin();
        let end = bitset.end();
        assert_ne!(begin, end);
        assert_ne!(end, begin);
    }

    #[test]
    fn bitset64_test_copy_bucket() {
        // Arrange
        let mut bitset: Bitset64<TestIndex> = Bitset64::new(TestIndex::new(100));
        let mut bitset2: Bitset64<TestIndex> = Bitset64::new(TestIndex::new(50));
        for i in indices(0, 100) {
            bitset.set_with_value(i, (i % 7) == 0);
        }
        for i in indices(0, 50) {
            bitset2.set_with_value(i, (i % 3) == 0);
        }
        // Act
        bitset.copy_bucket(&bitset2, TestIndex::new(0));
        // Assert
        for i in indices(0, 50) {
            assert_eq!(bitset[i], (i % 3) == 0);
        }
        for i in indices(50, 64) {
            assert!(!bitset[i]);
        }
        for i in indices(64, 100) {
            assert_eq!(bitset[i], (i % 7) == 0);
        }
    }

    #[test]
    fn bitset64_test_intersection() {
        // Arrange
        let mut bitset: Bitset64<TestIndex> = Bitset64::new(TestIndex::new(100));
        let mut bitset2: Bitset64<TestIndex> = Bitset64::new(TestIndex::new(50));
        for i in indices(0, 100) {
            bitset.set_with_value(i, (i % 7) == 0);
        }
        for i in indices(0, 50) {
            bitset2.set_with_value(i, (i % 3) == 0);
        }
        // Act
        bitset.intersection(&bitset2);
        // Assert
        for i in indices(0, 100) {
            assert_eq!(
                (i < 50) && ((i % 7) == 0) && ((i % 3) == 0),
                bitset.is_set(i)
            );
        }
    }

    #[test]
    fn bitset64_test_union_same_size() {
        // Arrange
        let mut bitset: Bitset64<TestIndex> = Bitset64::new(TestIndex::new(200));
        let mut bitset2: Bitset64<TestIndex> = Bitset64::new(TestIndex::new(200));
        bitset.set(TestIndex::new(30));
        bitset.set(TestIndex::new(99));
        bitset.set(TestIndex::new(100));
        bitset2.set(TestIndex::new(17));
        bitset2.set(TestIndex::new(100));
        bitset2.set(TestIndex::new(102));
        bitset2.set(TestIndex::new(199));
        // Act
        bitset.union(&bitset2);
        // Assert
        for i in indices(0, 200) {
            let v = i.value();
            if v == 30 || v == 99 || v == 100 || v == 17 || v == 102 || v == 199 {
                assert!(bitset.is_set(i), "i={v}");
            } else {
                assert!(!bitset.is_set(i), "i={v}");
            }
        }
    }

    #[test]
    fn bitset64_test_union_smaller() {
        // Arrange
        let mut bitset: Bitset64<TestIndex> = Bitset64::new(TestIndex::new(100));
        let mut bitset2: Bitset64<TestIndex> = Bitset64::new(TestIndex::new(200));
        bitset.set(TestIndex::new(30));
        bitset.set(TestIndex::new(99));
        bitset2.set(TestIndex::new(17));
        bitset2.set(TestIndex::new(30));
        bitset2.set(TestIndex::new(100));
        bitset2.set(TestIndex::new(102));
        bitset2.set(TestIndex::new(199));
        // Act
        bitset.union(&bitset2);
        // Assert
        for i in indices(0, 100) {
            let v = i.value();
            if v == 30 || v == 99 || v == 17 {
                assert!(bitset.is_set(i), "i={v}");
            } else {
                assert!(!bitset.is_set(i), "i={v}");
            }
        }
    }

    #[test]
    fn bitset64_test_union_larger() {
        // Arrange
        let mut bitset: Bitset64<TestIndex> = Bitset64::new(TestIndex::new(100));
        let mut bitset2: Bitset64<TestIndex> = Bitset64::new(TestIndex::new(200));
        bitset.set(TestIndex::new(30));
        bitset.set(TestIndex::new(99));
        bitset.set(TestIndex::new(17));
        bitset2.set(TestIndex::new(17));
        bitset2.set(TestIndex::new(100));
        bitset2.set(TestIndex::new(102));
        bitset2.set(TestIndex::new(199));
        // Act
        bitset2.union(&bitset);
        // Assert
        for i in indices(0, 200) {
            let v = i.value();
            if v == 30 || v == 99 || v == 100 || v == 17 || v == 102 || v == 199 {
                assert!(bitset2.is_set(i), "i={v}");
            } else {
                assert!(!bitset2.is_set(i), "i={v}");
            }
        }
    }

    #[test]
    fn bitset64_test_iterator_over_empty_bit_set() {
        // Arrange
        let bitset: Bitset64<TestIndex> = Bitset64::default();
        let mut num_bits = 0;
        // Act
        for index in &bitset {
            assert_eq!(index, TestIndex::new(-1)); // Never executed.
            num_bits += 1;
        }
        // Assert
        assert_eq!(num_bits, 0);
    }

    #[test]
    fn bitset64_test_iterator_over_random_bit_set() {
        // Arrange
        let k_size = TestIndex::new(100 * 1000);
        let mut bitset: Bitset64<TestIndex> = Bitset64::new(k_size);
        assert_eq!(k_size, bitset.size());

        let k_density: f32 = 0.5;
        let mut num_set_positions = 0;

        const K_RANDOM_SEED: u64 = 1;
        let mut randomizer = ChaCha8Rng::seed_from_u64(K_RANDOM_SEED);
        for i in indices(0, k_size) {
            // A float in the open interval (0, 1).
            let draw = ((randomizer.next_u32() >> 8) as f32 + 0.5) / (1u32 << 24) as f32;
            if draw < k_density {
                bitset.set(i);
                assert!(bitset.is_set(i));
                num_set_positions += 1;
            } else {
                assert!(!bitset.is_set(i));
            }
        }

        // Act
        let mut num_iterated_positions = 0;
        let mut old = TestIndex::new(-1);
        for index in &bitset {
            // Assert
            assert!(bitset.is_set(index));
            assert!(index > old);
            num_iterated_positions += 1;
            old = index;
        }
        assert_eq!(num_set_positions, num_iterated_positions);
    }

    // `std::is_trivially_copyable_v` is `Copy`.
    const fn assert_copy<T: Copy>() {}
    const _: () = assert_copy::<View<'static, i32>>();
    const _: () = assert_copy::<ConstView<'static, i32>>();
    const _: () = assert_copy::<View<'static, TestIndex>>();
    const _: () = assert_copy::<ConstView<'static, TestIndex>>();
    // Upstream asserts that a view is as large as `uint64_t*`. A Rust view holds a slice, a
    // pointer and a length, so the assertion is that the bounds checker adds nothing to it
    // in a release build.
    #[cfg(not(debug_assertions))]
    const _: () = {
        assert!(size_of::<View<'static, i32>>() == size_of::<&[Cell<u64>]>());
        assert!(size_of::<ConstView<'static, i32>>() == size_of::<&[Cell<u64>]>());
        assert!(size_of::<View<'static, TestIndex>>() == size_of::<&[Cell<u64>]>());
        assert!(size_of::<ConstView<'static, TestIndex>>() == size_of::<&[Cell<u64>]>());
    };

    #[test]
    fn bitset64_test_views() {
        // Arrange
        let bitset: Bitset64<TestIndex> = Bitset64::new(TestIndex::new(100));
        let mut view: View<'_, TestIndex> = bitset.view();
        let const_view: ConstView<'_, TestIndex> = bitset.const_view();

        // Act
        view.set(TestIndex::new(0));
        view.set(TestIndex::new(63));
        view.set(TestIndex::new(64));
        view.set(TestIndex::new(99));

        // Assert
        assert!(view[TestIndex::new(0)]);
        assert!(const_view[TestIndex::new(63)]);
        assert!(const_view[TestIndex::new(64)]);
        assert!(const_view[TestIndex::new(99)]);
        assert!(!const_view[TestIndex::new(1)]);
        assert!(!const_view[TestIndex::new(65)]);

        view.clear(TestIndex::new(63));
        assert!(!view[TestIndex::new(63)]);
        assert!(!const_view[TestIndex::new(63)]);
        assert!(!bitset.is_set(TestIndex::new(63)));
    }

    #[cfg(debug_assertions)]
    #[test]
    fn bitset64_death_test_view_bounds_checking() {
        // Arrange
        let empty: Bitset64<TestIndex> = Bitset64::default();
        // Act and Assert
        expect_death(
            || {
                let _ = empty.const_view()[TestIndex::new(0)];
            },
            "index < self.size",
        );
        expect_death(
            || {
                let _ = empty.view()[TestIndex::new(0)];
            },
            "index < self.size",
        );
        expect_death(|| empty.view().set(TestIndex::new(0)), "index < self.size");
        expect_death(
            || empty.view().clear(TestIndex::new(0)),
            "index < self.size",
        );

        // Size 10 allocates a single 64-bit bucket, so indices 10 and 63 lie within the
        // allocated word but outside [0, size).
        let bitset: Bitset64<TestIndex> = Bitset64::new(TestIndex::new(10));
        let mut view: View<'_, TestIndex> = bitset.view();
        let const_view: ConstView<'_, TestIndex> = bitset.const_view();

        expect_death(
            || {
                let _ = view[TestIndex::new(-1)];
            },
            "index >= ",
        );
        expect_death(
            || {
                let _ = view[TestIndex::new(10)];
            },
            "index < self.size",
        );
        expect_death(
            || {
                let _ = view[TestIndex::new(63)];
            },
            "index < self.size",
        );
        expect_death(
            || {
                let _ = const_view[TestIndex::new(-1)];
            },
            "index >= ",
        );
        expect_death(
            || {
                let _ = const_view[TestIndex::new(10)];
            },
            "index < self.size",
        );
        expect_death(
            || {
                let _ = const_view[TestIndex::new(63)];
            },
            "index < self.size",
        );

        expect_death(|| view.set(TestIndex::new(-1)), "index >= ");
        expect_death(|| view.set(TestIndex::new(10)), "index < self.size");
        expect_death(|| view.clear(TestIndex::new(-1)), "index >= ");
        expect_death(|| view.clear(TestIndex::new(10)), "index < self.size");
    }

    #[cfg(debug_assertions)]
    #[test]
    fn bitset64_death_test_member_bounds_checking() {
        // Arrange
        let bitset: Bitset64<TestIndex> = Bitset64::new(TestIndex::new(10));
        // Act and Assert
        expect_death(
            || {
                let _ = bitset[TestIndex::new(-1)];
            },
            "index >= ",
        );
        expect_death(
            || {
                let _ = bitset[TestIndex::new(10)];
            },
            "index < self.size",
        );
        expect_death(|| bitset.clone().set(TestIndex::new(-1)), "index >= ");
        expect_death(
            || bitset.clone().set(TestIndex::new(10)),
            "index < self.size",
        );
        expect_death(|| bitset.clone().clear(TestIndex::new(-1)), "index >= ");
        expect_death(
            || bitset.clone().clear(TestIndex::new(10)),
            "index < self.size",
        );
        expect_death(
            || bitset.clone().clear_bucket(TestIndex::new(-1)),
            "index >= ",
        );
        expect_death(
            || bitset.clone().clear_bucket(TestIndex::new(10)),
            "index < self.size",
        );

        let smaller: Bitset64<TestIndex> = Bitset64::new(TestIndex::new(5));
        expect_death(
            || bitset.clone().copy_bucket(&smaller, TestIndex::new(7)),
            "index < self.size",
        );
    }

    #[cfg(debug_assertions)]
    #[test]
    fn bitset64_death_test_negative_size() {
        // Arrange
        let bitset: Bitset64<TestIndex> = Bitset64::new(TestIndex::new(10));
        // Act and Assert
        expect_death(
            || drop(Bitset64::<TestIndex>::new(TestIndex::new(-1))),
            "size >= ",
        );
        expect_death(|| bitset.clone().resize(TestIndex::new(-1)), "size >= ");
        expect_death(
            || bitset.clone().clear_and_resize(TestIndex::new(-1)),
            "size >= ",
        );
    }

    #[test]
    fn bit_queue64_test_basic_test() {
        // Arrange
        const K_SIZE: i32 = 100000;
        let mut queue = BitQueue64::new(K_SIZE);

        const K_RANDOM_SEED: u64 = 1;
        let mut randomizer = ChaCha8Rng::seed_from_u64(K_RANDOM_SEED);
        let mut positions: Vec<i32> = Vec::new();
        for _ in indices(0, K_SIZE as i64) {
            let pos = uniform(&mut randomizer, 0, K_SIZE);
            queue.set(pos);
            positions.push(pos);
        }
        positions.sort_unstable();
        positions.dedup();
        // Act and Assert
        while let Some(&last) = positions.last() {
            assert_eq!(queue.top(), last);
            positions.pop();
            queue.clear_top();
        }
        assert_eq!(queue.top(), -1);
    }

    #[test]
    fn bit_queue64_test_set_all_up_to() {
        // Arrange
        let mut queue = BitQueue64::new(100);
        // Act
        queue.set_all_before(71);
        // Assert
        for pos in (0..=70).rev() {
            assert_eq!(queue.top(), pos);
            queue.clear_top();
        }
        assert_eq!(queue.top(), -1);
    }
}
