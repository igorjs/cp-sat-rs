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
// Ported from ortools/util/sorted_interval_list.h at commit c931f5f7. Changed: translated from C++ to Rust.
// Ported from ortools/util/sorted_interval_list.cc at commit c931f5f7. Changed: translated from C++ to Rust.

//! Sets of integers as sorted lists of disjoint closed intervals.

use core::cmp::Ordering;
use core::fmt;
use core::ops::Index;

use crate::saturated_arithmetic::{add_overflows, cap_add, cap_prod, cap_sub};

/// Represents a closed interval [start, end]. We must have start <= end.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub struct ClosedInterval {
    /// Inclusive.
    pub start: i64,
    /// Inclusive.
    pub end: i64,
}

/// An iterator over the values of a ClosedInterval object.
///
/// The iterator is designed to be very efficient, using just a single counter.
/// It works correctly for any combination of `start` and `end` except the full
/// int64_t range (start == INT64_MIN && end == INT64_MAX).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ClosedIntervalIterator {
    // Unsigned, so that the counter wraps for an interval that ends at `i64::MAX`.
    current: u64,
}

impl ClosedInterval {
    /// The interval [v, v].
    pub fn new_value(v: i64) -> Self {
        ClosedInterval { start: v, end: v }
    }

    pub fn new(s: i64, e: i64) -> Self {
        debug_assert!(s <= e, "Invalid ClosedInterval({s}, {e})");
        ClosedInterval { start: s, end: e }
    }

    pub fn debug_string(&self) -> String {
        if self.start == self.end {
            return format!("[{}]", self.start);
        }
        format!("[{},{}]", self.start, self.end)
    }

    /// `operator<` (sorted_interval_list.h:73): orders by `start` only, since disjoint
    /// intervals never share a start. It is not a `PartialOrd`, because equality compares
    /// `end` too.
    pub fn less_than(&self, other: &Self) -> bool {
        self.start < other.start
    }
}

impl fmt::Display for ClosedInterval {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.debug_string())
    }
}

impl ClosedIntervalIterator {
    pub fn current(&self) -> i64 {
        self.current as i64
    }

    pub fn pre_increment(&mut self) -> &mut Self {
        self.current = self.current.wrapping_add(1);
        self
    }

    pub fn post_increment(&mut self) {
        self.current = self.current.wrapping_add(1);
    }

    pub fn begin(interval: ClosedInterval) -> Self {
        Self::assert_not_full_int64_range(interval);
        ClosedIntervalIterator {
            current: interval.start as u64,
        }
    }

    pub fn end(interval: ClosedInterval) -> Self {
        Self::assert_not_full_int64_range(interval);
        ClosedIntervalIterator {
            current: (interval.end as u64).wrapping_add(1),
        }
    }

    // Triggers a DCHECK-failure when `interval` represents the full int64_t range.
    fn assert_not_full_int64_range(interval: ClosedInterval) {
        debug_assert_ne!(
            interval.start as u64,
            (interval.end as u64).wrapping_add(1),
            "Iteration over the full int64_t range is not supported."
        );
    }
}

/// The values of a [`ClosedInterval`] in order: the `begin()` to `end()` range of upstream.
#[derive(Clone, Copy, Debug)]
pub struct ClosedIntervalValues {
    current: ClosedIntervalIterator,
    end: ClosedIntervalIterator,
}

impl Iterator for ClosedIntervalValues {
    type Item = i64;

    fn next(&mut self) -> Option<i64> {
        if self.current == self.end {
            return None;
        }
        let value = self.current.current();
        self.current.post_increment();
        Some(value)
    }
}

impl IntoIterator for ClosedInterval {
    type Item = i64;
    type IntoIter = ClosedIntervalValues;

    fn into_iter(self) -> ClosedIntervalValues {
        ClosedIntervalValues {
            current: ClosedIntervalIterator::begin(self),
            end: ClosedIntervalIterator::end(self),
        }
    }
}

/// The text of `operator<<` on a `std::vector<ClosedInterval>`, as `"[1,4][6]"`.
pub fn intervals_to_string(intervals: &[ClosedInterval]) -> String {
    intervals_as_string(intervals)
}

/// Returns true iff we have:
/// - The intervals appear in increasing order.
/// - for all i: intervals[i].start <= intervals[i].end
///   (should always be true, by construction, but bad intervals can in practice
///   escape detection in opt mode).
/// - for all i but the last: intervals[i].end + 1 < intervals[i+1].start
pub fn intervals_are_sorted_and_non_adjacent(intervals: &[ClosedInterval]) -> bool {
    for i in 1..intervals.len() {
        let prev = &intervals[i - 1];
        let curr = &intervals[i];
        if prev.start > prev.end {
            return false;
        }
        // First test make sure that prev.end + 1 will not overflow.
        if prev.end == i64::MAX || prev.end + 1 >= curr.start {
            return false;
        }
    }
    match intervals.last() {
        None => true,
        Some(last) => last.start <= last.end,
    }
}

fn intervals_as_string<'a>(intervals: impl IntoIterator<Item = &'a ClosedInterval>) -> String {
    let mut result = String::new();
    for interval in intervals {
        result += &interval.debug_string();
    }
    if result.is_empty() {
        result = "[]".to_string();
    }
    result
}

fn sort_and_remove_invalid_intervals(intervals: &mut Vec<ClosedInterval>) {
    intervals.retain(|interval| interval.start <= interval.end);
    intervals.sort_unstable_by_key(|interval| interval.start);
}

// Transforms a sorted list of intervals in a sorted DISJOINT list for which
// IntervalsAreSortedAndNonAdjacent() would return true.
fn union_of_sorted_intervals(intervals: &mut Vec<ClosedInterval>) {
    debug_assert!(intervals.is_sorted_by_key(|interval| interval.start));
    let size = intervals.len();
    if size == 0 {
        return;
    }

    let mut new_size = 1;
    for i in 1..size {
        let current = intervals[i];
        let end = intervals[new_size - 1].end;
        if end == i64::MAX || current.start <= end + 1 {
            intervals[new_size - 1].end = current.end.max(end);
            continue;
        }
        intervals[new_size] = current;
        new_size += 1;
    }
    intervals.truncate(new_size);

    // This is important for InlinedVector in the case the result is a single
    // intervals.
    intervals.shrink_to_fit();
    debug_assert!(intervals_are_sorted_and_non_adjacent(intervals));
}

// TODO(user): Use MathUtil::CeilOfRatio / FloorOfRatio instead.
fn ceil_ratio(value: i64, positive_coeff: i64) -> i64 {
    debug_assert!(positive_coeff > 0);
    let result = value / positive_coeff;
    let adjust = i64::from(result * positive_coeff < value);
    result + adjust
}

fn floor_ratio(value: i64, positive_coeff: i64) -> i64 {
    debug_assert!(positive_coeff > 0);
    let result = value / positive_coeff;
    let adjust = i64::from(result * positive_coeff > value);
    result - adjust
}

// `CapProd(x, abs_coeff)` of sorted_interval_list.cc:585-586. For coeff == kint64min the
// optimised build takes `abs_coeff` as non-negative, so an overflow is capped by the sign of
// `x` alone.
fn cap_prod_by_abs(x: i64, abs_coeff: i64) -> i64 {
    x.checked_mul(abs_coeff)
        .unwrap_or(if x < 0 { i64::MIN } else { i64::MAX })
}

/// We call *domain* any subset of Int64 = [kint64min, kint64max].
///
/// This class can be used to represent such set efficiently as a sorted and
/// non-adjacent list of intervals. This is efficient as long as the size of such
/// list stays reasonable.
///
/// In the comments below, the domain of *this will always be written 'D'.
/// Note that all the functions are safe with respect to integer overflow.
#[derive(Clone, Debug, Default, PartialEq, Eq, Hash)]
pub struct Domain {
    // Invariant: will always satisfy IntervalsAreSortedAndNonAdjacent().
    intervals: Vec<ClosedInterval>,
}

/// Allows to iterate over all values of a domain in order, with `Domain::values()`.
#[derive(Clone, Debug)]
pub struct DomainIterator<'a> {
    value: i64,
    intervals: &'a [ClosedInterval],
    index: usize,
}

impl<'a> DomainIterator<'a> {
    pub fn new(intervals: &'a [ClosedInterval]) -> Self {
        DomainIterator {
            value: intervals.first().map_or(0, |interval| interval.start),
            intervals,
            index: 0,
        }
    }
}

impl Iterator for DomainIterator<'_> {
    type Item = i64;

    fn next(&mut self) -> Option<i64> {
        if self.index == self.intervals.len() {
            return None;
        }
        let value = self.value;
        if self.value == self.intervals[self.index].end {
            self.index += 1;
            if self.index != self.intervals.len() {
                self.value = self.intervals[self.index].start;
            }
        } else {
            self.value += 1;
        }
        Some(value)
    }
}

// Some functions relax the domain when its "complexity" (i.e NumIntervals())
// become too large.
const K_DOMAIN_COMPLEXITY_LIMIT: i32 = 100;

impl Domain {
    /// Constructor for the common case of a singleton domain.
    pub fn new_value(value: i64) -> Self {
        Domain {
            intervals: vec![ClosedInterval::new(value, value)],
        }
    }

    /// Constructor for the common case of a single interval [left, right].
    /// If left > right, this will result in the empty domain.
    pub fn new(left: i64, right: i64) -> Self {
        let mut intervals = vec![ClosedInterval {
            start: left,
            end: right,
        }];
        if left > right {
            intervals.clear();
        }
        Domain { intervals }
    }

    /// Returns the full domain Int64.
    pub fn all_values() -> Self {
        Domain::new(i64::MIN, i64::MAX)
    }

    /// Returns the domain [int_min, value].
    pub fn lower_or_equal(value: i64) -> Self {
        Domain::new(i64::MIN, value)
    }

    /// Returns the domain [value., int_max].
    pub fn greater_or_equal(value: i64) -> Self {
        Domain::new(value, i64::MAX)
    }

    /// Creates a domain from the union of an unsorted list of integer values.
    /// Input values may be repeated, with no consequence on the output
    pub fn from_values(mut values: Vec<i64>) -> Self {
        values.sort_unstable();
        let mut result = Domain::default();
        for v in values {
            match result.intervals.last_mut() {
                Some(last) if v <= last.end + 1 => last.end = v,
                _ => result.intervals.push(ClosedInterval::new(v, v)),
            }
            if result
                .intervals
                .last()
                .is_some_and(|last| last.end == i64::MAX)
            {
                break;
            }
        }
        result
    }

    /// Creates a domain from the union of an unsorted list of intervals.
    ///
    /// Note that invalid intervals (start > end) will log a DFATAL error and will
    /// be ignored.
    pub fn from_intervals(intervals: &[ClosedInterval]) -> Self {
        let mut result = Domain {
            intervals: intervals.to_vec(),
        };
        sort_and_remove_invalid_intervals(&mut result.intervals);
        union_of_sorted_intervals(&mut result.intervals);
        result
    }

    /// Same as FromIntervals() for a flattened representation (start, end,
    /// start, end, ...).
    ///
    /// Note that invalid intervals (start > end) will log a DFATAL error and will
    /// be ignored.
    pub fn from_flat_span_of_intervals(flat_intervals: &[i64]) -> Self {
        debug_assert!(
            flat_intervals.len().is_multiple_of(2),
            "{}",
            flat_intervals.len()
        );
        let mut result = Domain::default();
        result.intervals.reserve(flat_intervals.len() / 2);
        for pair in flat_intervals.as_chunks::<2>().0 {
            result.intervals.push(ClosedInterval::new(pair[0], pair[1]));
        }
        sort_and_remove_invalid_intervals(&mut result.intervals);
        union_of_sorted_intervals(&mut result.intervals);
        result
    }

    /// Allows building a Domain object from a list of intervals, each one a
    /// single value (`[5]`) or two increasing values (`[8, 10]`).
    ///
    /// Invalid intervals (start > end) will log a DFATAL error and will be
    /// ignored.
    pub fn from_vector_intervals(intervals: &[Vec<i64>]) -> Self {
        let mut result = Domain::default();
        for interval in intervals {
            if interval.len() == 1 {
                result
                    .intervals
                    .push(ClosedInterval::new(interval[0], interval[0]));
            } else {
                debug_assert_eq!(interval.len(), 2);
                result
                    .intervals
                    .push(ClosedInterval::new(interval[0], interval[1]));
            }
        }
        sort_and_remove_invalid_intervals(&mut result.intervals);
        union_of_sorted_intervals(&mut result.intervals);
        result
    }

    /// Allows building a Domain object from a flattened list of intervals,
    /// `[0, 2, 5, 5, 8, 10]`.
    ///
    /// Note that invalid intervals (start > end) will log a DFATAL error and will
    /// be ignored.
    pub fn from_flat_intervals(flat_intervals: &[i64]) -> Self {
        Domain::from_flat_span_of_intervals(flat_intervals)
    }

    /// Returns the flattened list of interval bounds of the domain.
    ///
    /// Thus the domain {0, 1, 2, 5, 8, 9, 10} will return [0, 2, 5, 5, 8, 10].
    pub fn flattened_intervals(&self) -> Vec<i64> {
        let mut result = Vec::new();
        for interval in &self.intervals {
            result.push(interval.start);
            result.push(interval.end);
        }
        result
    }

    /// Allows to iterate over all values of a domain in order with
    /// `for v in domain.values() { ... }`.
    ///
    /// Upstream has a second form for a temporary domain that moves the intervals into the
    /// range. A temporary here lives to the end of the `for` statement, so it needs none.
    pub fn values(&self) -> DomainIterator<'_> {
        DomainIterator::new(&self.intervals)
    }

    /// Returns true if this is the empty set.
    pub fn is_empty(&self) -> bool {
        self.intervals.is_empty()
    }

    /// Returns the number of elements in the domain. It is capped at kint64max
    pub fn size(&self) -> i64 {
        let mut size = 0;
        for interval in &self.intervals {
            size = cap_add(size, cap_sub(interval.end, interval.start));
        }
        // Because the intervals are closed on both side above, with miss 1 per
        // interval.
        cap_add(size, self.intervals.len() as i64)
    }

    /// Returns true if the domain has just two values. This often mean a non-fixed
    /// Boolean variable.
    pub fn has_two_values(&self) -> bool {
        if self.intervals.len() == 1 {
            // Upstream adds 1 to `i64::MAX` here, which is undefined behaviour in C++ and
            // wraps in practice (sorted_interval_list.h:291).
            return self.intervals[0].end == self.intervals[0].start.wrapping_add(1);
        } else if self.intervals.len() == 2 {
            return self.intervals[0].end == self.intervals[0].start
                && self.intervals[1].end == self.intervals[1].start;
        }
        false
    }

    /// Returns the min value of the domain.
    /// The domain must not be empty.
    pub fn min(&self) -> i64 {
        debug_assert!(!self.is_empty());
        self.intervals[0].start
    }

    /// Returns the max value of the domain.
    /// The domain must not be empty.
    pub fn max(&self) -> i64 {
        debug_assert!(!self.is_empty());
        self.intervals[self.intervals.len() - 1].end
    }

    /// Returns the value closest to zero. If there is a tie, pick positive one.
    pub fn smallest_value(&self) -> i64 {
        debug_assert!(!self.is_empty());
        let mut result = self.min();
        for interval in &self.intervals {
            if interval.start <= 0 && interval.end >= 0 {
                return 0;
            }
            for b in [interval.start, interval.end] {
                // sorted_interval_list.cc:255-256: an optimised build compares the magnitudes
                // exactly, also for kint64min, where std::abs and the unary minus overflow.
                if b > 0 && b.unsigned_abs() <= result.unsigned_abs() {
                    result = b;
                }
                if b < 0 && b.unsigned_abs() < result.unsigned_abs() {
                    result = b;
                }
            }
        }
        result
    }

    /// Returns the value closest to the given point.
    /// If there is a tie, pick larger one.
    // TODO(user): Use std::upper_bound() like in ValueAtOrBefore() ?
    pub fn closest_value(&self, wanted: i64) -> i64 {
        debug_assert!(!self.is_empty());
        if wanted <= self.intervals[0].start {
            return self.intervals[0].start;
        }
        // Both are set by the first interval before they are read, since `wanted` is above
        // the start of the first interval.
        let mut best_point: i64 = 0;
        let mut best_distance: i64 = 0;
        for interval in &self.intervals {
            if interval.start <= wanted && wanted <= interval.end {
                return wanted;
            } else if interval.start >= wanted {
                return if cap_sub(interval.start, wanted) <= best_distance {
                    interval.start
                } else {
                    best_point
                };
            } else {
                best_point = interval.end;
                best_distance = cap_sub(wanted, interval.end);
            }
        }
        best_point
    }

    /// Returns the closest value in the domain that is <= to the input.
    /// Do not change the input if there is no such value.
    pub fn value_at_or_before(&self, input: i64) -> i64 {
        // Because we only compare by start and there is no duplicate starts, this
        // should be the next interval after the one that has a chance to contains
        // value.
        let it = self
            .intervals
            .partition_point(|interval| interval.start <= input);
        if it == 0 {
            return input;
        }
        let it = it - 1;
        if input <= self.intervals[it].end {
            input
        } else {
            self.intervals[it].end
        }
    }

    /// Returns the closest value in the domain that is >= to the input.
    /// Do not change the input if there is no such value.
    pub fn value_at_or_after(&self, input: i64) -> i64 {
        // Because we only compare by start and there is no duplicate starts, this
        // should be the next interval after the one that has a chance to contains
        // value.
        let it = self
            .intervals
            .partition_point(|interval| interval.start <= input);
        if it == self.intervals.len() {
            return input;
        }
        let candidate = self.intervals[it].start;
        if it == 0 {
            return candidate;
        }
        if input <= self.intervals[it - 1].end {
            input
        } else {
            candidate
        }
    }

    /// If the domain contains zero, this return the simple interval around it.
    /// Otherwise, this returns an empty domain.
    pub fn part_around_zero(&self) -> Domain {
        for interval in &self.intervals {
            if interval.start <= 0 && interval.end >= 0 {
                return Domain::new(interval.start, interval.end);
            }
        }
        Domain::default()
    }

    /// Returns true iff the domain is reduced to a single value.
    /// The domain must not be empty.
    pub fn is_fixed(&self) -> bool {
        self.min() == self.max()
    }

    /// Returns the value of a fixed domain. IsFixed() must be true.
    /// This is the same as Min() or Max() but allows for a more readable code and
    /// also crash in debug mode if called on a non fixed domain.
    pub fn fixed_value(&self) -> i64 {
        debug_assert!(self.is_fixed());
        self.intervals[0].start
    }

    /// Returns true iff value is in Domain.
    pub fn contains(&self, value: i64) -> bool {
        // Because we only compare by start and there is no duplicate starts, this
        // should be the next interval after the one that has a chance to contains
        // value.
        let it = self
            .intervals
            .partition_point(|interval| interval.start <= value);
        if it == 0 {
            return false;
        }
        value <= self.intervals[it - 1].end
    }

    /// Returns the distance from the value to the domain.
    // TODO(user): Deal with overflow.
    pub fn distance(&self, value: i64) -> i64 {
        let mut min_distance = i64::MAX;
        for interval in &self.intervals {
            if value >= interval.start && value <= interval.end {
                return 0;
            }
            if interval.start > value {
                // sorted_interval_list.cc:339 and 342 wrap on overflow.
                min_distance = min_distance.min(interval.start.wrapping_sub(value));
                break;
            } else {
                min_distance = value.wrapping_sub(interval.end);
            }
        }
        min_distance
    }

    /// Returns true iff D is included in the given domain.
    pub fn is_included_in(&self, domain: &Domain) -> bool {
        let mut i = 0;
        let others = &domain.intervals;
        for interval in &self.intervals {
            // Find the unique interval in others that contains interval if any.
            while i < others.len() && interval.end > others[i].end {
                i += 1;
            }
            if i == others.len() {
                return false;
            }
            if interval.start < others[i].start {
                return false;
            }
        }
        true
    }

    /// Returns true iff D overlaps with the given domain, that is, the
    /// intersection of the two domains is not empty.
    pub fn overlaps_with(&self, domain: &Domain) -> bool {
        let a = &self.intervals;
        let b = &domain.intervals;
        let (mut i, mut j) = (0, 0);
        while i < a.len() && j < b.len() {
            if a[i].start <= b[j].start {
                if a[i].end < b[j].start {
                    // Empty intersection. We advance past the first interval.
                    i += 1;
                } else {
                    // a[i].end >= b[j].start
                    return true;
                }
            } else {
                // a[i].start > b[i].start.
                // We do the exact same thing as above, but swapping a and b.
                if b[j].end < a[i].start {
                    j += 1;
                } else {
                    // b[j].end >= a[i].start
                    return true;
                }
            }
        }
        false
    }

    /// Returns the set Int64 ∖ D.
    #[must_use]
    pub fn complement(&self) -> Domain {
        let mut result = Domain::default();
        let mut next_start = i64::MIN;
        result.intervals.reserve(self.intervals.len() + 1);
        for interval in &self.intervals {
            if interval.start != i64::MIN {
                result
                    .intervals
                    .push(ClosedInterval::new(next_start, interval.start - 1));
            }
            if interval.end == i64::MAX {
                return result;
            }
            next_start = interval.end + 1;
        }
        result
            .intervals
            .push(ClosedInterval::new(next_start, i64::MAX));
        debug_assert!(intervals_are_sorted_and_non_adjacent(&result.intervals));
        result
    }

    /// Returns {x ∈ Int64, ∃ e ∈ D, x = -e}.
    ///
    /// Note in particular that if the negation of Int64 is not Int64 but
    /// Int64 \ {kint64min} !!
    #[must_use]
    pub fn negation(&self) -> Domain {
        let mut result = self.clone();
        result.negate_in_place();
        result
    }

    /// Returns the intersection of D and domain.
    #[must_use]
    pub fn intersection_with(&self, domain: &Domain) -> Domain {
        let mut result = Domain::default();
        let a = &self.intervals;
        let b = &domain.intervals;
        let (mut i, mut j) = (0, 0);
        while i < a.len() && j < b.len() {
            if a[i].start <= b[j].start {
                if a[i].end < b[j].start {
                    // Empty intersection. We advance past the first interval.
                    i += 1;
                } else {
                    // a[i].end >= b[j].start
                    // Non-empty intersection: push back the intersection of these two, and
                    // advance past the first interval to finish.
                    if a[i].end <= b[j].end {
                        result
                            .intervals
                            .push(ClosedInterval::new(b[j].start, a[i].end));
                        i += 1;
                    } else {
                        // a[i].end > b[j].end.
                        result
                            .intervals
                            .push(ClosedInterval::new(b[j].start, b[j].end));
                        j += 1;
                    }
                }
            } else {
                // a[i].start > b[i].start.
                // We do the exact same thing as above, but swapping a and b.
                if b[j].end < a[i].start {
                    j += 1;
                } else {
                    // b[j].end >= a[i].start
                    if b[j].end <= a[i].end {
                        result
                            .intervals
                            .push(ClosedInterval::new(a[i].start, b[j].end));
                        j += 1;
                    } else {
                        // a[i].end > b[j].end.
                        result
                            .intervals
                            .push(ClosedInterval::new(a[i].start, a[i].end));
                        i += 1;
                    }
                }
            }
        }
        debug_assert!(intervals_are_sorted_and_non_adjacent(&result.intervals));
        result
    }

    /// Returns the union of D and domain.
    #[must_use]
    pub fn union_with(&self, domain: &Domain) -> Domain {
        let mut result = Domain::default();
        let a = &self.intervals;
        let b = &domain.intervals;
        result.intervals.reserve(a.len() + b.len());
        // A stable merge, as `std::merge`: an element of `b` goes first only if it is
        // strictly smaller than the next element of `a`.
        let (mut i, mut j) = (0, 0);
        while i < a.len() && j < b.len() {
            if b[j].start < a[i].start {
                result.intervals.push(b[j]);
                j += 1;
            } else {
                result.intervals.push(a[i]);
                i += 1;
            }
        }
        result.intervals.extend_from_slice(&a[i..]);
        result.intervals.extend_from_slice(&b[j..]);
        union_of_sorted_intervals(&mut result.intervals);
        result
    }

    /// Returns {x ∈ Int64, ∃ a ∈ D, ∃ b ∈ domain, x = a + b}.
    // TODO(user): Use a better algorithm.
    #[must_use]
    pub fn addition_with(&self, domain: &Domain) -> Domain {
        let mut result = Domain::default();

        let a = &self.intervals;
        let b = &domain.intervals;
        result.intervals.reserve(a.len() * b.len());
        for i in a {
            for j in b {
                if i.start > 0 && j.start > 0 {
                    if add_overflows(i.start, j.start) {
                        continue; // empty.
                    }
                    result.intervals.push(ClosedInterval::new(
                        i.start + j.start,
                        cap_add(i.end, j.end),
                    ));
                } else if i.end < 0 && j.end < 0 {
                    if add_overflows(i.end, j.end) {
                        continue; // empty.
                    }
                    result.intervals.push(ClosedInterval::new(
                        cap_add(i.start, j.start),
                        i.end + j.end,
                    ));
                } else {
                    result.intervals.push(ClosedInterval::new(
                        cap_add(i.start, j.start),
                        cap_add(i.end, j.end),
                    ));
                }
            }
        }

        // The sort is not needed if one of the list is of size 1.
        if a.len() > 1 && b.len() > 1 {
            result
                .intervals
                .sort_unstable_by_key(|interval| interval.start);
        }
        union_of_sorted_intervals(&mut result.intervals);
        result
    }

    /// Returns {x ∈ Int64, ∃ e ∈ D, x = e * coeff}.
    ///
    /// Note that because the resulting domain will only contains multiple of
    /// coeff, the size of intervals.size() can become really large. If it is
    /// larger than a fixed constant, exact will be set to false and the result
    /// will be set to ContinuousMultiplicationBy(coeff).
    ///
    /// Note that if you multiply by a negative coeff, kint64min will be dropped
    /// from the result even if it was here due to how this is implemented.
    #[must_use]
    pub fn multiplication_by(&self, coeff: i64, exact: Option<&mut bool>) -> Domain {
        let mut exact = exact;
        if let Some(exact) = exact.as_deref_mut() {
            *exact = true;
        }
        if self.intervals.is_empty() {
            return Domain::default();
        }
        if coeff == 0 {
            return Domain::new_value(0);
        }

        // sorted_interval_list.cc:549: std::abs(kint64min) is kint64min, which an optimised
        // build still treats as larger than 1.
        let abs_coeff = coeff.wrapping_abs();
        let abs_coeff_is_above_one = coeff.unsigned_abs() > 1;
        let size_if_non_trivial = if abs_coeff_is_above_one {
            self.size()
        } else {
            0
        };
        if size_if_non_trivial > i64::from(K_DOMAIN_COMPLEXITY_LIMIT) {
            if let Some(exact) = exact {
                *exact = false;
            }
            return self.continuous_multiplication_by(coeff);
        }

        let mut result = Domain::default();
        if abs_coeff_is_above_one {
            let max_value = i64::MAX / abs_coeff;
            let min_value = i64::MIN / abs_coeff;
            result.intervals.reserve(size_if_non_trivial as usize);
            for i in &self.intervals {
                let mut v = i.start;
                loop {
                    // We ignore anything that overflow.
                    if v >= min_value && v <= max_value {
                        // Because abs_coeff > 1, all new values are disjoint.
                        let new_value = v * abs_coeff;
                        result
                            .intervals
                            .push(ClosedInterval::new(new_value, new_value));
                    }

                    // This is to avoid doing ++v when v is kint64max!
                    if v == i.end {
                        break;
                    }
                    v += 1;
                }
            }
        } else {
            result = self.clone();
        }
        if coeff < 0 {
            result.negate_in_place();
        }
        result
    }

    /// If NumIntervals() is too large, this return a superset of the domain.
    #[must_use]
    pub fn relax_if_too_complex(&self) -> Domain {
        if self.num_intervals() > K_DOMAIN_COMPLEXITY_LIMIT {
            Domain::new(self.min(), self.max())
        } else {
            self.clone()
        }
    }

    /// Returns a superset of MultiplicationBy() to avoid the explosion in the
    /// representation size. This behaves as if we replace the set D of
    /// non-adjacent integer intervals by the set of floating-point elements in the
    /// same intervals.
    ///
    /// For instance, [1, 100] * 2 will be transformed in [2, 200] and not in
    /// [2][4][6]...[200] like in MultiplicationBy().
    #[must_use]
    pub fn continuous_multiplication_by(&self, coeff: i64) -> Domain {
        let mut result = self.clone();
        // sorted_interval_list.cc:583
        let abs_coeff = coeff.wrapping_abs();
        for i in &mut result.intervals {
            i.start = cap_prod_by_abs(i.start, abs_coeff);
            i.end = cap_prod_by_abs(i.end, abs_coeff);
        }
        union_of_sorted_intervals(&mut result.intervals);
        if coeff < 0 {
            result.negate_in_place();
        }
        result
    }

    /// Same as `continuous_multiplication_by` with a domain as the factor.
    #[must_use]
    pub fn continuous_multiplication_by_domain(&self, domain: &Domain) -> Domain {
        let mut result = Domain::default();
        for i in &self.intervals {
            for j in &domain.intervals {
                let a = cap_prod(i.start, j.start);
                let b = cap_prod(i.end, j.end);
                let c = cap_prod(i.start, j.end);
                let d = cap_prod(i.end, j.start);
                let new_interval = ClosedInterval {
                    start: a.min(b).min(c).min(d),
                    end: a.max(b).max(c).max(d),
                };
                result.intervals.push(new_interval);
            }
        }
        result
            .intervals
            .sort_unstable_by_key(|interval| interval.start);
        union_of_sorted_intervals(&mut result.intervals);
        result
    }

    /// Returns {x ∈ Int64, ∃ e ∈ D, x = e / coeff}.
    ///
    /// For instance Domain(1, 7).DivisionBy(2) == Domain(0, 3).
    #[must_use]
    pub fn division_by(&self, coeff: i64) -> Domain {
        assert_ne!(coeff, 0);
        let mut result = self.clone();
        // sorted_interval_list.cc:615
        let abs_coeff = coeff.wrapping_abs();
        for i in &mut result.intervals {
            i.start /= abs_coeff;
            i.end /= abs_coeff;
        }
        union_of_sorted_intervals(&mut result.intervals);
        if coeff < 0 {
            result.negate_in_place();
        }
        result
    }

    /// Returns {x ∈ Int64, ∃ e ∈ D, x * coeff = e}.
    ///
    /// For instance Domain(1, 7).InverseMultiplicationBy(2) == Domain(1, 3).
    #[must_use]
    pub fn inverse_multiplication_by(&self, coeff: i64) -> Domain {
        if coeff == 0 {
            return if self.contains(0) {
                Domain::all_values()
            } else {
                Domain::default()
            };
        }
        let mut result = self.clone();
        let mut new_size = 0;
        // sorted_interval_list.cc:631
        let abs_coeff = coeff.wrapping_abs();
        for index in 0..result.intervals.len() {
            let i = result.intervals[index];
            let start = ceil_ratio(i.start, abs_coeff);
            let end = floor_ratio(i.end, abs_coeff);
            if start > end {
                continue;
            }
            if new_size > 0 && start == result.intervals[new_size - 1].end + 1 {
                result.intervals[new_size - 1].end = end;
            } else {
                result.intervals[new_size] = ClosedInterval::new(start, end);
                new_size += 1;
            }
        }
        result.intervals.truncate(new_size);
        result.intervals.shrink_to_fit();
        debug_assert!(intervals_are_sorted_and_non_adjacent(&result.intervals));
        if coeff < 0 {
            result.negate_in_place();
        }
        result
    }

    /// Returns a superset of {x ∈ Int64, ∃ e ∈ D, ∃ m ∈ modulo, x = e % m }.
    ///
    /// We check that modulo is strictly positive.
    /// The sign of the modulo depends on the sign of e.
    /// We compute the exact min/max if the modulo is fixed, otherwise we will
    /// just return a superset.
    #[must_use]
    pub fn positive_modulo_by_superset(&self, modulo: &Domain) -> Domain {
        if self.is_empty() {
            return Domain::default();
        }
        assert!(modulo.min() > 0);
        let max_mod = modulo.max() - 1;
        if self.max() >= 0 && self.min() <= 0 {
            return Domain::new(self.min().max(-max_mod), self.max().min(max_mod));
        }
        if self.min() > 0 {
            return modulo_helper(self.min(), self.max(), modulo);
        }
        debug_assert!(self.max() < 0);
        // sorted_interval_list.cc:680: the negation wraps for kint64min.
        modulo_helper(self.max().wrapping_neg(), self.min().wrapping_neg(), modulo).negation()
    }

    /// Returns a superset of:
    /// {x ∈ Int64, ∃ e ∈ D, ∃ d ∈ divisor \ {0}, x = e / d }.
    #[must_use]
    pub fn division_by_superset(&self, divisor: &Domain) -> Domain {
        if self.is_empty() || divisor.is_empty() {
            return Domain::default();
        }
        // The division takes its extreme values either on the extreme values of the
        // dividend and the extreme values of the divisor or the closest the divisor
        // can be to zero.
        let positive_superset = |lmin: i64, lmax: i64, rmin: i64, rmax: i64| {
            debug_assert!(rmin > 0);
            debug_assert!(rmin <= rmax);
            Domain::new(
                (lmin / rmax).min(lmin / rmin),
                (lmax / rmin).max(lmax / rmax),
            )
        };
        let negative_superset = |lmin: i64, lmax: i64, rmin: i64, rmax: i64| {
            debug_assert!(rmax < 0);
            debug_assert!(rmin <= rmax);
            let safe_div = |a: i64, b: i64| {
                if a == i64::MIN && b == -1 {
                    i64::MAX
                } else {
                    a / b
                }
            };
            Domain::new(
                safe_div(lmax, rmin).min(safe_div(lmax, rmax)),
                safe_div(lmin, rmax).max(safe_div(lmin, rmin)),
            )
        };
        if divisor.min() == 0 && divisor.max() == 0 {
            return Domain::default();
        }
        if divisor.min() >= 0 {
            return positive_superset(self.min(), self.max(), divisor.min().max(1), divisor.max());
        }
        if divisor.max() <= 0 {
            return negative_superset(self.min(), self.max(), divisor.min(), divisor.max().min(-1));
        }
        negative_superset(
            self.min(),
            self.max(),
            divisor.min(),
            divisor.value_at_or_before(-1),
        )
        .union_with(&positive_superset(
            self.min(),
            self.max(),
            divisor.value_at_or_after(1),
            divisor.max(),
        ))
    }

    /// Returns a superset of {x ∈ Int64, ∃ y ∈ D, x = y * y }.
    #[must_use]
    pub fn square_superset(&self) -> Domain {
        if self.is_empty() {
            return Domain::default();
        }
        let abs_domain = self
            .intersection_with(&Domain::new(0, i64::MAX))
            .union_with(&self.negation().intersection_with(&Domain::new(0, i64::MAX)));
        if abs_domain.size() >= i64::from(K_DOMAIN_COMPLEXITY_LIMIT) {
            let mut result = Domain::default();
            result.intervals.reserve(abs_domain.intervals.len());
            for interval in &abs_domain {
                result.intervals.push(ClosedInterval::new(
                    cap_prod(interval.start, interval.start),
                    cap_prod(interval.end, interval.end),
                ));
            }
            union_of_sorted_intervals(&mut result.intervals);
            result
        } else {
            let mut values = Vec::with_capacity(abs_domain.size() as usize);
            for value in abs_domain.values() {
                values.push(cap_prod(value, value));
            }
            Domain::from_values(values)
        }
    }

    /// Returns a superset of {x ∈ Int64, ∃ y ∈ D, x = (a*y + b)*(c*y + d) }.
    #[must_use]
    pub fn quadratic_superset(&self, a: i64, b: i64, c: i64, d: i64) -> Domain {
        if self.is_empty() {
            return Domain::default();
        }

        if self.size() < i64::from(K_DOMAIN_COMPLEXITY_LIMIT) {
            let mut values = Vec::with_capacity(self.size() as usize);
            for value in self.values() {
                values.push(cap_prod(
                    cap_add(cap_prod(a, value), b),
                    cap_add(cap_prod(c, value), d),
                ));
            }
            return Domain::from_values(values);
        }

        if a == 0 {
            return self
                .multiplication_by(cap_prod(c, b), None)
                .addition_with(&Domain::new_value(cap_prod(d, b)));
        }
        if c == 0 {
            return self
                .multiplication_by(cap_prod(a, d), None)
                .addition_with(&Domain::new_value(cap_prod(d, b)));
        }

        let mut result = Domain::default();
        result.intervals.reserve(self.intervals.len());
        for interval in &self.intervals {
            result.intervals.push(evaluate_quadratic_prod_interval(
                a,
                b,
                c,
                d,
                interval.start,
                interval.end,
            ));
        }
        result
            .intervals
            .sort_unstable_by_key(|interval| interval.start);
        union_of_sorted_intervals(&mut result.intervals);
        result
    }

    /// Advanced usage. Given some *implied* information on this domain that is
    /// assumed to be always true (i.e. only values in the intersection with
    /// implied domain matter), this function will simplify the current domain
    /// without changing the set of "possible values".
    ///
    /// More precisely, this will:
    ///  - Take the intersection with implied_domain.
    ///  - Minimize the number of intervals. For example, if the
    ///    domain is [1,2][4] and implied is [1][4], then the domain can be
    ///    relaxed to [1, 4] to simplify its complexity without changing the set
    ///    of admissible value assuming only implied values can be seen.
    ///  - Restrict as much as possible the bounds of the remaining intervals.
    ///    For example, if the input is [1,2] and implied is [0,4], then the domain
    ///    will not be changed.
    ///
    /// Note that `domain.SimplifyUsingImpliedDomain(domain)` will just return
    /// [domain.Min(), domain.Max()]. This is meant to be applied to the right-hand
    /// side of a constraint to make its propagation more efficient.
    // It is a bit difficult to see, but this code is doing the same thing as
    // for all interval in this.UnionWith(implied_domain.Complement())):
    //  - Take the two extreme points (min and max) in interval \inter implied.
    //  - Append to result [min, max] if these points exists.
    #[must_use]
    pub fn simplify_using_implied_domain(&self, implied_domain: &Domain) -> Domain {
        let mut result = Domain::default();
        if implied_domain.is_empty() {
            return result;
        }

        let mut i = 0;
        // Both are set before they are read, when `started` becomes true.
        let mut min_point: i64 = 0;
        let mut max_point: i64 = 0;
        let mut started = false;
        for interval in &self.intervals {
            // We only "close" the new result interval if it cannot be extended by
            // implied_domain.Complement(). The only extension possible look like:
            // interval_:    ...]   [....
            // implied :   ...]       [...  i  ...]
            if started && implied_domain.intervals[i].start < interval.start {
                result
                    .intervals
                    .push(ClosedInterval::new(min_point, max_point));
                started = false;
            }

            // Find the two extreme points in interval \inter implied_domain.
            // Always stop the loop at the first interval with and end strictly greater
            // that interval.end.
            while i < implied_domain.intervals.len() {
                let current = implied_domain.intervals[i];
                if current.end >= interval.start && current.start <= interval.end {
                    // Current and interval have a non-empty intersection.
                    let inter_max = interval.end.min(current.end);
                    if !started {
                        started = true;
                        min_point = interval.start.max(current.start);
                        max_point = inter_max;
                    } else {
                        // No need to update the min_point here, and the new inter_max must
                        // necessarily be > old one.
                        debug_assert!(inter_max >= max_point);
                        max_point = inter_max;
                    }
                }
                if current.end > interval.end {
                    break;
                }
                i += 1;
            }
            if i == implied_domain.intervals.len() {
                break;
            }
        }
        if started {
            result
                .intervals
                .push(ClosedInterval::new(min_point, max_point));
        }
        debug_assert!(intervals_are_sorted_and_non_adjacent(&result.intervals));
        result
    }

    /// If this \ other is a singleton, returns it. Returns None otherwise.
    ///
    /// This is similar to checking if this.IntersectionWith(other.Complement()) is
    /// non-empty and fixed, but avoid allocating memory.
    pub fn unique_value_not_in(&self, other: &Domain) -> Option<i64> {
        let mut result: Option<i64> = None;
        let mut i = 0;
        let to_exclude = &other.intervals;
        for &ClosedInterval { mut start, end } in &self.intervals {
            while start <= end {
                // find the first interval with to_exclude[i].end >= start.
                while i < to_exclude.len() && to_exclude[i].end < start {
                    i += 1;
                }
                if i < to_exclude.len() {
                    debug_assert!(to_exclude[i].end >= start);
                    if to_exclude[i].start <= start {
                        if to_exclude[i].end == i64::MAX {
                            return result;
                        }
                        start = to_exclude[i].end + 1;
                        continue;
                    }
                    // Notice that if end > start, there will be no overflow here.
                    if end == start || to_exclude[i].start == start + 1 {
                        if result.is_some() {
                            return None;
                        }
                        result = Some(start);
                        if to_exclude[i].end == i64::MAX {
                            return result;
                        }
                        start = to_exclude[i].end + 1;
                        continue;
                    }
                    return None;
                } else if end == start {
                    if result.is_some() {
                        return None;
                    }
                    result = Some(start);
                    break; // Next interval.
                } else {
                    return None;
                }
            }
        }
        result
    }

    /// Basic read-only std::vector<> wrapping to view a Domain as a sorted list of
    /// non-adjacent intervals. Note that we don't expose size() which might be
    /// confused with the number of values in the domain.
    pub fn num_intervals(&self) -> i32 {
        self.intervals.len() as i32
    }

    pub fn front(&self) -> ClosedInterval {
        self.intervals[0]
    }

    pub fn back(&self) -> ClosedInterval {
        self.intervals[self.intervals.len() - 1]
    }

    // Same as Negation() but modify the current domain.
    fn negate_in_place(&mut self) {
        if self.intervals.is_empty() {
            return;
        }
        self.intervals.reverse();
        if self.intervals[self.intervals.len() - 1].end == i64::MIN {
            // corner-case
            self.intervals.pop();
        }
        for r in &mut self.intervals {
            core::mem::swap(&mut r.start, &mut r.end);
            r.start = if r.start == i64::MIN {
                i64::MAX
            } else {
                -r.start
            };
            r.end = if r.end == i64::MIN { i64::MAX } else { -r.end };
        }
        debug_assert!(intervals_are_sorted_and_non_adjacent(&self.intervals));
    }
}

// `operator[]` of upstream, which takes an `int`.
impl Index<i32> for Domain {
    type Output = ClosedInterval;

    fn index(&self, i: i32) -> &ClosedInterval {
        &self.intervals[i as usize]
    }
}

// `begin()` and `end()` of upstream, for a range `for` loop.
impl<'a> IntoIterator for &'a Domain {
    type Item = &'a ClosedInterval;
    type IntoIter = core::slice::Iter<'a, ClosedInterval>;

    fn into_iter(self) -> Self::IntoIter {
        self.intervals.iter()
    }
}

// Lexicographic order on the intervals() representation. `Ord` is not implemented, since
// its `min` and `max` would hide the methods of the same name that give the bounds.
impl PartialOrd for Domain {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        let d1 = &self.intervals;
        let d2 = &other.intervals;
        let common_size = d1.len().min(d2.len());
        for i in 0..common_size {
            let i1 = &d1[i];
            let i2 = &d2[i];
            if i1.start < i2.start {
                return Some(Ordering::Less);
            }
            if i1.start > i2.start {
                return Some(Ordering::Greater);
            }
            if i1.end < i2.end {
                return Some(Ordering::Less);
            }
            if i1.end > i2.end {
                return Some(Ordering::Greater);
            }
        }
        Some(d1.len().cmp(&d2.len()))
    }
}

// `ToString()` of upstream is `to_string()` through this: a compact string of a vector of
// intervals like "[1,4][6][10,20]".
impl fmt::Display for Domain {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&intervals_as_string(&self.intervals))
    }
}

fn modulo_helper(min: i64, max: i64, modulo: &Domain) -> Domain {
    debug_assert!(min > 0);
    debug_assert!(modulo.min() > 0);
    let max_mod = modulo.max() - 1;

    // The min/max are exact if the modulo is fixed. Note that we could return the
    // exact domain with a potential hole but we currently don't.
    if modulo.min() == modulo.max() {
        // sorted_interval_list.cc:658-661: `max` is kint64min when the caller negated
        // kint64min, so the subtraction and the sum wrap. An optimised build takes `min` as
        // positive, so the remainder is unsigned.
        let size = max.wrapping_sub(min);
        let v1 = (min as u64 % modulo.max() as u64) as i64;
        if v1.wrapping_add(size) > max_mod {
            return Domain::new(0, max_mod);
        }
        return Domain::new(v1, v1.wrapping_add(size));
    }

    // TODO(user): This is a superset.
    Domain::new(0, max.min(max_mod))
}

// Implementation of QuadraticSuperset() for a single interval.
fn evaluate_quadratic_prod_interval(
    a: i64,
    b: i64,
    c: i64,
    d: i64,
    variable_min: i64,
    variable_max: i64,
) -> ClosedInterval {
    // We have (a*x + b)(c*x + d) = a*c*x*x + (a*d + b*c)*x + b*d
    // The minimum or maximum is at x = -(a*d + b*c)/(2*a*c)
    //
    // The minimum and maximum of the expression happens when x is one of the
    // following:
    // - variable_min;
    // - variable_max;
    // - the closest domain value lower or equal to the parabola extreme;
    // - the closest domain value higher than the parabola extreme.

    let nominator: i128 = -i128::from(a) * i128::from(d) - i128::from(b) * i128::from(c);
    let denominator: i128 = i128::from(a) * i128::from(c);
    let evaluated_minimum_point = (nominator / denominator) / 2;

    let evaluate = |x: i64| cap_prod(cap_add(cap_prod(a, x), b), cap_add(cap_prod(c, x), d));

    let at_min_x = evaluate(variable_min);
    let at_max_x = evaluate(variable_max);
    let mut min_var = at_min_x.min(at_max_x);
    let mut max_var = at_min_x.max(at_max_x);

    let point_at_minimum_64 = evaluated_minimum_point as i64;
    let rounder: i64 = if (nominator > 0) == (denominator > 0) {
        1
    } else {
        -1
    };

    if point_at_minimum_64 >= variable_min && point_at_minimum_64 <= variable_max {
        let point1 = evaluate(point_at_minimum_64);
        min_var = min_var.min(point1);
        max_var = max_var.max(point1);
    }

    // sorted_interval_list.cc:787: wraps when the point is kint64max.
    let point2_x = point_at_minimum_64.wrapping_add(rounder);
    if point2_x >= variable_min && point2_x <= variable_max {
        let point2 = evaluate(point2_x);
        min_var = min_var.min(point2);
        max_var = max_var.max(point2);
    }

    ClosedInterval::new(min_var, max_var)
}

/// Returns the sum of smallest k values in the domain.
pub fn sum_of_k_min_value_in_domain(domain: &Domain, k: i32) -> i64 {
    let mut current_sum: i64 = 0;
    let mut current_index: i32 = 0;
    for interval in domain {
        if current_index >= k {
            break;
        }
        // Upstream narrows the bound to an `int` here (sorted_interval_list.cc:914).
        // The counter is an i64: an optimised build does not wrap at i32::MAX, and the loop
        // goes on up to the interval end.
        let mut v = i64::from(interval.start as i32);
        while v <= interval.end {
            if current_index >= k {
                break;
            }
            current_index += 1;
            current_sum += v;
            v += 1;
        }
    }
    current_sum
}

/// Returns the sum of largest k values in the domain.
pub fn sum_of_k_max_value_in_domain(domain: &Domain, k: i32) -> i64 {
    -sum_of_k_min_value_in_domain(&domain.negation(), k)
}

/// This class represents a sorted list of disjoint, closed intervals.  When an
/// interval is inserted, all intervals that overlap it or are adjacent to it are
/// merged into one. I.e. [0,14] and [15,30] will be merged to [0,30].
///
/// Upstream keeps the intervals in a `std::set` and hands out its iterators. A position
/// in a sorted vector plays that part here, since stable Rust has no cursor on a
/// `BTreeSet`. As upstream, a position is invalidated by non-const operations.
// TODO(user): Templatize the class on the type of the bounds.
#[derive(Clone, Debug, Default)]
pub struct SortedDisjointIntervalList {
    // Sorted by `(start, end)`, the `IntervalComparator` of upstream.
    intervals: Vec<ClosedInterval>,
}

/// The position of an interval of a [`SortedDisjointIntervalList`], as `IntervalSet::iterator`.
/// `end()` is the position past the last interval.
pub type SortedDisjointIntervalListIterator = usize;

impl SortedDisjointIntervalList {
    pub fn new_from_intervals(intervals: &[ClosedInterval]) -> Self {
        let mut list = SortedDisjointIntervalList::default();
        for interval in intervals {
            list.insert_interval(interval.start, interval.end);
        }
        list
    }

    /// Creates a SortedDisjointIntervalList and fills it with intervals
    /// [starts[i]..ends[i]]. All intervals must be consistent (starts[i] <=
    /// ends[i]). There are two version, one for int64_t and one for int.
    pub fn new(starts: &[i64], ends: &[i64]) -> Self {
        let mut list = SortedDisjointIntervalList::default();
        list.insert_intervals(starts, ends);
        list
    }

    pub fn new_i32(starts: &[i32], ends: &[i32]) -> Self {
        let mut list = SortedDisjointIntervalList::default();
        list.insert_intervals_i32(starts, ends);
        list
    }

    /// Builds the complement of the interval list on the interval [start, end].
    // Upstream is not `const` here, and does not change the list either.
    pub fn build_complement_on_interval(&self, start: i64, end: i64) -> Self {
        let mut interval_list = SortedDisjointIntervalList::default();
        let mut next_start = start;
        let mut it = self.first_interval_greater_or_equal(start);
        while it != self.end() {
            let interval = self.intervals[it];
            let next_end = cap_sub(interval.start, 1);
            if next_end > end {
                break;
            }
            if next_start <= next_end {
                interval_list.insert_interval(next_start, next_end);
            }
            next_start = cap_add(interval.end, 1);
            it += 1;
        }
        if next_start <= end {
            interval_list.insert_interval(next_start, end);
        }
        interval_list
    }

    /// Adds the interval [start..end] to the list, and merges overlapping or
    /// immediately adjacent intervals ([2, 5] and [6, 7] are adjacent, but
    /// [2, 5] and [7, 8] are not).
    ///
    /// Returns the position of the inserted interval (possibly merged with others).
    ///
    /// If start > end, it does LOG(DFATAL) and returns end() (no interval added).
    pub fn insert_interval(&mut self, start: i64, end: i64) -> SortedDisjointIntervalListIterator {
        // start > end could mean an empty interval, but we prefer to LOG(DFATAL)
        // anyway. Really, the user should never give us that.
        if start > end {
            let invalid = ClosedInterval { start, end };
            if cfg!(debug_assertions) {
                panic!("Invalid interval: {invalid}");
            }
            log::error!("Invalid interval: {invalid}");
            return self.end();
        }

        let inserted = ClosedInterval { start, end };
        let position = self
            .intervals
            .partition_point(|interval| (interval.start, interval.end) < (start, end));
        if self.intervals.get(position) == Some(&inserted) {
            return position; // Duplicate: exit immediately.
        }
        self.intervals.insert(position, inserted);

        // TODO(user): tune the algorithm below if it proves to be a bottleneck.
        // For example, one could try to avoid an insertion if it's not needed
        // (when the interval merges with a single existing interval or is fully
        // contained by one).

        // Iterate over the previous positions whose end is after (or almost at) our
        // start. After this, "it1" will point to the first interval that needs to be
        // merged with the current interval (possibly pointing to the current interval
        // itself, if no "earlier" interval should be merged).
        let mut it1 = position;
        if start == i64::MIN {
            // Catch underflows
            it1 = 0;
        } else {
            let before_start = start - 1;
            while it1 != 0 {
                let prev_it = it1 - 1;
                if self.intervals[prev_it].end < before_start {
                    break;
                }
                it1 = prev_it;
            }
        }

        // Ditto, on the other side: "it2" will point to the interval *after* the last
        // one that should be merged with the current interval.
        let mut it2 = position;
        if end == i64::MAX {
            it2 = self.end();
        } else {
            let after_end = end + 1;
            loop {
                it2 += 1;
                if !(it2 != self.end() && self.intervals[it2].start <= after_end) {
                    break;
                }
            }
        }

        // [it1..it2) is the range (inclusive on it1, exclusive on it2) of intervals
        // that should be merged together. We'll set it3 = it2-1 and erase [it1..it3)
        // and set *it3 to the merged interval.
        let it3 = it2 - 1;
        if it1 == it3 {
            return it3; // Nothing was merged.
        }
        let new_start = self.intervals[it1].start.min(start);
        let new_end = self.intervals[it3].end.max(end);
        self.intervals.drain(it1..it3);
        let it = it1;
        self.intervals[it].start = new_start;
        self.intervals[it].end = new_end;
        it
    }

    fn insert_all<T: Copy + Into<i64>>(&mut self, starts: &[T], ends: &[T]) {
        assert_eq!(starts.len(), ends.len());
        for (&start, &end) in starts.iter().zip(ends) {
            self.insert_interval(start.into(), end.into());
        }
    }

    /// Adds all intervals [starts[i]..ends[i]].
    ///
    /// Same behavior as InsertInterval() upon invalid intervals. There's a version
    /// with int64_t and int32_t.
    pub fn insert_intervals(&mut self, starts: &[i64], ends: &[i64]) {
        self.insert_all(starts, ends);
    }

    pub fn insert_intervals_i32(&mut self, starts: &[i32], ends: &[i32]) {
        // TODO(user): treat kint32min and kint32max as their kint64 variants.
        self.insert_all(starts, ends);
    }

    /// Returns the number of disjoint intervals in the list.
    pub fn num_intervals(&self) -> i32 {
        self.intervals.len() as i32
    }

    /// Returns the position of the first interval containing or above the given value.
    /// Returns end() if no interval fulfills that condition.
    ///
    /// If the value is within an interval, both functions will return it.
    pub fn first_interval_greater_or_equal(
        &self,
        value: i64,
    ) -> SortedDisjointIntervalListIterator {
        let it = self
            .intervals
            .partition_point(|interval| interval.start <= value);
        if it == self.begin() {
            return it;
        }
        let it_prev = it - 1;
        debug_assert!(self.intervals[it_prev].start <= value);
        if self.intervals[it_prev].end >= value {
            it_prev
        } else {
            it
        }
    }

    /// Returns the position of the last interval containing or below the given value.
    /// Returns end() if no interval fulfills that condition.
    pub fn last_interval_less_or_equal(&self, value: i64) -> SortedDisjointIntervalListIterator {
        let it = self
            .intervals
            .partition_point(|interval| interval.start <= value);
        if it == self.begin() {
            return self.end();
        }
        it - 1
    }

    pub fn debug_string(&self) -> String {
        let mut str = String::new();
        for interval in &self.intervals {
            str += &interval.debug_string();
        }
        str
    }

    /// The position of the first interval.
    pub fn begin(&self) -> SortedDisjointIntervalListIterator {
        0
    }

    /// The position past the last interval.
    pub fn end(&self) -> SortedDisjointIntervalListIterator {
        self.intervals.len()
    }

    /// The interval at a position that is not `end()`, as `*it`.
    pub fn at(&self, position: SortedDisjointIntervalListIterator) -> &ClosedInterval {
        &self.intervals[position]
    }

    /// Returns a reference to the last interval. The list must not be empty.
    pub fn last(&self) -> &ClosedInterval {
        self.intervals
            .last()
            .expect("last() needs a list that is not empty")
    }

    pub fn clear(&mut self) {
        self.intervals.clear();
    }

    pub fn swap(&mut self, other: &mut SortedDisjointIntervalList) {
        core::mem::swap(&mut self.intervals, &mut other.intervals);
    }
}

// Const iterators for SortedDisjoinIntervalList, for a range `for` loop.
impl<'a> IntoIterator for &'a SortedDisjointIntervalList {
    type Item = &'a ClosedInterval;
    type IntoIter = core::slice::Iter<'a, ClosedInterval>;

    fn into_iter(self) -> Self::IntoIter {
        self.intervals.iter()
    }
}

#[cfg(test)]
mod tests {
    use cpsat_testutil::{expect_death, expect_debug_death};
    use rand_chacha::ChaCha8Rng;
    use rand_chacha::rand_core::{RngCore, SeedableRng};

    use super::*;

    // The intervals of a list of pairs, each one made by `ClosedInterval::new`, as the
    // braces of the C++ initialiser lists are.
    fn intervals(pairs: &[(i64, i64)]) -> Vec<ClosedInterval> {
        pairs
            .iter()
            .map(|&(start, end)| ClosedInterval::new(start, end))
            .collect()
    }

    // A draw in [lo, hi), as `absl::Uniform(random, lo, hi)`.
    fn uniform(random: &mut ChaCha8Rng, lo: i64, hi: i64) -> i64 {
        lo + (random.next_u64() % (hi - lo) as u64) as i64
    }

    // True with probability `p`, as `absl::Bernoulli(random, p)`.
    fn bernoulli(random: &mut ChaCha8Rng, p: f64) -> bool {
        ((random.next_u64() >> 11) as f64) / ((1u64 << 53) as f64) < p
    }

    #[test]
    #[cfg(debug_assertions)]
    fn closed_interval_constructor_test_d_fatal_in_non_opt_if_start_greater_than_end() {
        // Arrange
        // Act
        // Assert
        expect_debug_death(
            || {
                ClosedInterval::new(1, 0);
            },
            "Invalid",
        );
        expect_debug_death(
            || {
                ClosedInterval::new(-55, -56);
            },
            "Invalid",
        );
        expect_debug_death(
            || {
                ClosedInterval::new(i64::MAX, i64::MIN);
            },
            "Invalid",
        );
    }

    #[test]
    fn closed_intervaliterator_set_test_range_loop() {
        // Arrange
        let k_intervals = [
            ClosedInterval::new(0, 0),
            ClosedInterval::new(0, 1),
            ClosedInterval::new(1000, 1000),
            ClosedInterval::new(0, 100),
            ClosedInterval::new(-10, 20),
            ClosedInterval::new(i64::MAX, i64::MAX),
            ClosedInterval::new(i64::MAX - 30, i64::MAX),
            ClosedInterval::new(i64::MIN, i64::MIN),
            ClosedInterval::new(i64::MIN, i64::MIN + 3),
        ];
        for interval in k_intervals {
            // Act
            let mut had_at_least_one_iteration = false;
            let mut last_value: i64 = 0;
            for value in interval {
                // Assert
                if !had_at_least_one_iteration {
                    assert_eq!(value, interval.start, "interval: {interval}");
                    had_at_least_one_iteration = true;
                } else {
                    assert_eq!(value - last_value, 1, "interval: {interval}");
                }
                last_value = value;
            }
            assert!(had_at_least_one_iteration, "interval: {interval}");
            assert_eq!(last_value, interval.end, "interval: {interval}");
        }
    }

    #[test]
    fn intervals_are_sorted_and_non_adjacent_test_basic_tests() {
        // Arrange
        // Act
        // Assert
        assert!(intervals_are_sorted_and_non_adjacent(&[]));
        assert!(intervals_are_sorted_and_non_adjacent(&intervals(&[(1, 1)])));
        assert!(intervals_are_sorted_and_non_adjacent(&intervals(&[
            (1, 1),
            (3, 6)
        ])));
        assert!(!intervals_are_sorted_and_non_adjacent(&intervals(&[
            (3, 6),
            (1, 5)
        ])));
        assert!(!intervals_are_sorted_and_non_adjacent(&intervals(&[
            (1, 1),
            (2, 6)
        ])));
        assert!(!intervals_are_sorted_and_non_adjacent(&intervals(&[
            (1, 2),
            (3, 6)
        ])));
    }

    #[test]
    #[cfg(not(debug_assertions))]
    fn invalid_intervals_test_behaves_as_empty_interval_in_opt_mode() {
        // Arrange
        // Act
        // Assert
        assert!(ClosedInterval::new(1, 0).less_than(&ClosedInterval::new(3, 2)));
        assert!(!intervals_are_sorted_and_non_adjacent(&intervals(&[(
            1, 0
        )])));
        assert_eq!(
            "[]",
            Domain::new(0, 10)
                .intersection_with(&Domain::new(50, -40))
                .to_string()
        );
    }

    #[test]
    fn intervals_are_sorted_and_non_adjacent_test_no_overflow() {
        // Arrange
        // Act
        // Assert
        assert!(intervals_are_sorted_and_non_adjacent(&intervals(&[(
            i64::MIN,
            i64::MAX
        )])));
        assert!(intervals_are_sorted_and_non_adjacent(&intervals(&[
            (i64::MIN, i64::MIN),
            (i64::MAX, i64::MAX)
        ])));
        assert!(!intervals_are_sorted_and_non_adjacent(&intervals(&[
            (i64::MIN, i64::MAX),
            (i64::MAX, i64::MAX)
        ])));
        assert!(!intervals_are_sorted_and_non_adjacent(&intervals(&[
            (i64::MIN, i64::MIN),
            (i64::MIN, i64::MAX)
        ])));
    }

    #[test]
    fn domain_test_to_string_test() {
        // Arrange
        // Act
        // Assert
        assert_eq!("[]", Domain::from_values(vec![]).to_string());
        assert_eq!("[4]", Domain::from_values(vec![4]).to_string());
        assert_eq!("[-4]", Domain::from_values(vec![-4]).to_string());
        assert_eq!(
            "[1,9]",
            Domain::from_values(vec![1, 2, 3, 4, 5, 9, 8, 7, 6]).to_string()
        );
        assert_eq!(
            "[1,5][7,9]",
            Domain::from_values(vec![9, 8, 7, 1, 2, 3, 4, 5]).to_string()
        );
        assert_eq!(
            "[1,3][5][7][9,10]",
            Domain::from_values(vec![1, 2, 3, 5, 7, 9, 10]).to_string()
        );
        assert_eq!(
            "[1,3]",
            Domain::from_values(vec![1, 1, 3, 1, 2, 3, 2, 3]).to_string()
        );
    }

    #[test]
    fn domain_test_from_intervals() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            "[1,5][7,9]",
            Domain::from_intervals(&intervals(&[(1, 5), (7, 9)])).to_string()
        );
        expect_debug_death(
            || {
                assert_eq!(
                    "[1,5]",
                    Domain::from_intervals(&intervals(&[(1, 5), (9, 7)])).to_string()
                );
            },
            "Invalid",
        );
        expect_debug_death(
            || {
                assert_eq!(
                    "[]",
                    Domain::from_intervals(&intervals(&[(5, 1), (9, 7)])).to_string()
                );
            },
            "Invalid",
        );
    }

    #[test]
    fn domain_test_from_vector_intervals() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            "[1,5][7,9]",
            Domain::from_vector_intervals(&[vec![1, 5], vec![7, 9]]).to_string()
        );
        expect_debug_death(
            || {
                assert_eq!(
                    "[1,5]",
                    Domain::from_vector_intervals(&[vec![1, 5], vec![9, 7]]).to_string()
                );
            },
            "Invalid",
        );
        expect_debug_death(
            || {
                assert_eq!(
                    "[]",
                    Domain::from_vector_intervals(&[vec![5, 1], vec![9, 7]]).to_string()
                );
            },
            "Invalid",
        );
    }

    #[test]
    fn domain_test_from_flat_span_of_intervals() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            "[1,5][7,9]",
            Domain::from_flat_span_of_intervals(&[1, 5, 7, 9]).to_string()
        );
        expect_debug_death(
            || {
                assert_eq!(
                    "[1,5]",
                    Domain::from_flat_span_of_intervals(&[1, 5, 9, 7]).to_string()
                );
            },
            "Invalid",
        );
        expect_debug_death(
            || {
                assert_eq!(
                    "[]",
                    Domain::from_flat_span_of_intervals(&[5, 1, 9, 7]).to_string()
                );
            },
            "Invalid",
        );
    }

    #[test]
    fn domain_test_le_or_ge() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            "[1,9223372036854775807]",
            Domain::greater_or_equal(1).to_string()
        );
        assert_eq!(
            "[-9223372036854775808,3]",
            Domain::lower_or_equal(3).to_string()
        );
    }

    #[test]
    fn domain_test_contains() {
        // Arrange
        let d = Domain::from_intervals(&intervals(&[(-4, -3), (0, 0), (7, i64::MAX)]));

        // Act
        // Assert
        assert!(d.contains(-4));
        assert!(d.contains(-3));
        assert!(d.contains(0));
        assert!(d.contains(7));
        assert!(d.contains(10));
        assert!(d.contains(i64::MAX));
        assert!(!d.contains(-7));
        assert!(!d.contains(-5));
        assert!(!d.contains(-1));
        assert!(!d.contains(1));
        assert!(!d.contains(6));
    }

    #[test]
    fn domain_test_distance() {
        // Arrange
        let d = Domain::from_intervals(&intervals(&[(-4, -3), (0, 0), (7, i64::MAX)]));

        // Act
        // Assert
        assert_eq!(d.distance(-4), 0);
        assert_eq!(d.distance(-8), 4);
        assert_eq!(d.distance(-3), 0);
        assert_eq!(d.distance(-2), 1);
        assert_eq!(d.distance(-1), 1);
        assert_eq!(d.distance(0), 0);
        assert_eq!(d.distance(1), 1);
        assert_eq!(d.distance(2), 2);
        assert_eq!(d.distance(3), 3);
        assert_eq!(d.distance(4), 3);
        assert_eq!(d.distance(5), 2);
        assert_eq!(d.distance(6), 1);
        assert_eq!(d.distance(7), 0);
        assert_eq!(d.distance(8), 0);
    }

    #[test]
    fn domain_test_value_iteration() {
        // Arrange
        let d = Domain::from_intervals(&intervals(&[(-4, -3), (0, 0), (7, 10)]));
        let mut test: Vec<i64> = Vec::new();

        // Act
        for v in d.values() {
            test.push(v);
        }

        // Assert
        assert_eq!(test, vec![-4, -3, 0, 7, 8, 9, 10]);
    }

    #[test]
    fn domain_test_empty_value_iteration() {
        // Arrange
        let mut test: Vec<i64> = Vec::new();

        // Act
        for v in Domain::default().values() {
            test.push(v);
        }

        // Assert
        assert!(test.is_empty());
    }

    #[test]
    fn domain_test_iteration_over_temporary_works() {
        // Arrange
        let mut test: Vec<i64> = Vec::new();

        // Act
        for v in Domain::from_values(vec![0, 1, 3, 5, 15, 27]).values() {
            test.push(v);
        }

        // Assert
        assert_eq!(test, vec![0, 1, 3, 5, 15, 27]);
    }

    #[test]
    fn domain_test_larger_domain_iteration() {
        // Arrange
        let mut num_values = 0;
        let mut sum: i64 = 0;

        // Act
        for v in Domain::new(1, 1000)
            .union_with(&Domain::new(2001, 4000))
            .values()
        {
            sum += v;
            num_values += 1;
        }

        // Assert
        assert_eq!(sum, 6501500); // Just to use v.
        assert_eq!(num_values, 3000);
    }

    #[test]
    fn domain_test_contains_large() {
        // Arrange
        let mut intervals: Vec<ClosedInterval> = Vec::new();
        for i in 0..100000 {
            intervals.push(ClosedInterval::new(3 * i, 3 * i + 1));
        }

        // Act
        let d = Domain::from_intervals(&intervals);

        // Assert
        for i in 0..300000 {
            assert_eq!(i % 3 != 2, d.contains(i));
        }
    }

    #[test]
    fn domain_test_size() {
        // Arrange
        // Act
        // Assert
        assert_eq!(4, Domain::new(1, 4).size());
        assert_eq!(
            3,
            Domain::from_intervals(&intervals(&[(-4, -3), (0, 0)])).size()
        );
        assert_eq!(i64::MAX - 6, Domain::new(7, i64::MAX).size());
        assert_eq!(
            i64::MAX - 3,
            Domain::from_intervals(&intervals(&[(-4, -3), (0, 0), (7, i64::MAX)])).size()
        );
        assert_eq!(i64::MAX, Domain::new(-5, i64::MAX - 2).size());
        assert_eq!(i64::MAX, Domain::new(i64::MIN + 4, 10).size());
    }

    #[test]
    fn domain_test_has_two_values() {
        // Arrange
        // Act
        // Assert
        assert!(!Domain::default().has_two_values());
        assert!(!Domain::new(1, 4).has_two_values());
        assert!(Domain::new(0, 1).has_two_values());
        assert!(Domain::new(1, 2).has_two_values());
        assert!(Domain::from_values(vec![1, 4]).has_two_values());
        assert!(!Domain::from_values(vec![1, 3, 4]).has_two_values());
    }

    #[test]
    fn domain_test_size_is_capped() {
        // Arrange
        let intervals: Vec<ClosedInterval> = vec![
            ClosedInterval::new(i64::MIN, -10),
            ClosedInterval::new(10, i64::MAX),
        ];

        // Act
        let domain = Domain::from_intervals(&intervals);

        // Assert
        assert_eq!(i64::MAX, domain.size());
    }

    #[test]
    fn domain_test_is_included_in() {
        // Arrange
        // Act
        // Assert
        assert!(Domain::new(0, 4).is_included_in(&Domain::new(0, 4)));
        assert!(!Domain::new(0, 4).is_included_in(&Domain::new(0, 3)));
        assert!(!Domain::new(0, 4).is_included_in(&Domain::new(1, 4)));
        assert!(
            Domain::from_values(vec![1, 4, 7]).is_included_in(&Domain::from_intervals(&intervals(
                &[(0, 2), (4, 4), (6, 9)]
            )))
        );
        assert!(
            Domain::from_values(vec![1, 7]).is_included_in(&Domain::from_intervals(&intervals(&[
                (0, 2),
                (4, 4),
                (6, 9)
            ])))
        );
        assert!(
            !Domain::from_values(vec![1, 5]).is_included_in(&Domain::from_intervals(&intervals(
                &[(0, 2), (4, 4), (6, 9)]
            )))
        );
    }

    #[test]
    fn domain_test_overlaps_with() {
        // Arrange
        // Act
        // Assert
        assert!(Domain::new(0, 4).overlaps_with(&Domain::new(0, 4)));
        assert!(!Domain::new(0, 2).overlaps_with(&Domain::new(3, 5)));
        assert!(
            !Domain::from_values(vec![1, 3, 5]).overlaps_with(&Domain::from_values(vec![0, 2, 4]))
        );
        assert!(
            Domain::from_values(vec![1, 2, 3, 5])
                .overlaps_with(&Domain::from_values(vec![0, 2, 4]))
        );
    }

    #[test]
    fn domain_test_intersection_with_empty_cases() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            "[]",
            Domain::default()
                .intersection_with(&Domain::default())
                .to_string()
        );
        assert_eq!(
            "[]",
            Domain::default()
                .intersection_with(&Domain::new(0, 10))
                .to_string()
        );
        assert_eq!(
            "[]",
            Domain::new(0, 10)
                .intersection_with(&Domain::default())
                .to_string()
        );
        assert_eq!(
            "[]",
            Domain::new(0, 10)
                .intersection_with(&Domain::new(11, 15))
                .to_string()
        );
    }

    #[test]
    fn domain_test_intersection_with_simple_cases() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            "[0]",
            Domain::new(-10, 0)
                .intersection_with(&Domain::new(0, 10))
                .to_string()
        );
        assert_eq!(
            "[0][10]",
            Domain::from_intervals(&intervals(&[(-10, 0), (10, 20)]))
                .intersection_with(&Domain::new(0, 10))
                .to_string()
        );
        assert_eq!(
            "[5,7]",
            Domain::new(5, 10)
                .intersection_with(&Domain::new(0, 7))
                .to_string()
        );
        assert_eq!(
            "[5,10]",
            Domain::new(5, 10)
                .intersection_with(&Domain::from_intervals(&intervals(&[(0, 15), (18, 18)])))
                .to_string()
        );
        assert_eq!(
            "[5,10]",
            Domain::new(0, 10)
                .intersection_with(&Domain::new(5, 15))
                .to_string()
        );
        assert_eq!(
            "[1][4][6]",
            Domain::new(0, 8)
                .intersection_with(&Domain::from_values(vec![1, 4, 6, 9]))
                .to_string()
        );
    }

    #[test]
    fn domain_test_intersection_with_list_of_values_case() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            "[0][5,6][8]",
            Domain::from_values(vec![-10, 0, 1, 5, 6, 7, 8, 10])
                .intersection_with(&Domain::from_values(vec![-1, 0, 3, 5, 6, 8, 9]))
                .to_string()
        );
    }

    #[test]
    fn domain_test_unique_value_not_in() {
        // Arrange
        // Act
        // Assert
        // Difference is {0, 5} (multiple values) -> returns None
        assert_eq!(
            Domain::new(0, 5).unique_value_not_in(&Domain::new(1, 4)),
            None
        );

        // Difference is {0} (exactly one value) -> returns 0
        assert_eq!(
            Domain::new(0, 5).unique_value_not_in(&Domain::new(1, 7)),
            Some(0)
        );

        // Difference is {8} (exactly one value) -> returns 8
        assert_eq!(
            Domain::from_values(vec![2, 8]).unique_value_not_in(&Domain::new(1, 7)),
            Some(8)
        );

        // Fully covered / Subset: difference is empty -> returns None
        assert_eq!(
            Domain::new(2, 3).unique_value_not_in(&Domain::new(1, 5)),
            None
        );

        // Identical domains: difference is empty -> returns None
        assert_eq!(
            Domain::new(1, 5).unique_value_not_in(&Domain::new(1, 5)),
            None
        );

        // Exactly one value at the upper bound not covered
        assert_eq!(
            Domain::new(1, 5).unique_value_not_in(&Domain::new(1, 4)),
            Some(5)
        );

        // Exactly one value at the lower bound not covered
        assert_eq!(
            Domain::new(1, 5).unique_value_not_in(&Domain::new(2, 5)),
            Some(1)
        );

        // Disjoint domain with a single value -> returns that value
        assert_eq!(
            Domain::new(0, 0).unique_value_not_in(&Domain::new(1, 5)),
            Some(0)
        );

        // Disjoint domain with multiple values -> returns None
        assert_eq!(
            Domain::new(8, 9).unique_value_not_in(&Domain::new(1, 5)),
            None
        );

        // Fragmented Domain (built from values) with exactly one value not in other
        assert_eq!(
            Domain::from_values(vec![1, 5, 9]).unique_value_not_in(&Domain::new(0, 7)),
            Some(9)
        );

        // Fragmented Domain with multiple values not in other ({1, 9})
        assert_eq!(
            Domain::from_values(vec![1, 5, 9]).unique_value_not_in(&Domain::new(3, 7)),
            None
        );

        // Empty domain vs empty domain (assuming Domain() or similar creates an empty
        // domain)
        assert_eq!(
            Domain::default().unique_value_not_in(&Domain::default()),
            None
        );
    }

    #[test]
    fn domain_unique_value_not_in_overflow_case() {
        // Arrange
        // Act
        // Assert
        // 1. Excluded interval ends exactly at kint64max.
        assert_eq!(
            Domain::new(i64::MAX - 5, i64::MAX)
                .unique_value_not_in(&Domain::new(i64::MAX - 5, i64::MAX)),
            None
        );

        // 2. The only remaining value is exactly kint64max.
        assert_eq!(
            Domain::new(i64::MAX - 5, i64::MAX)
                .unique_value_not_in(&Domain::new(i64::MAX - 5, i64::MAX - 1)),
            Some(i64::MAX)
        );

        // 3. The remaining value is right next to kint64max.
        assert_eq!(
            Domain::new(i64::MAX - 1, i64::MAX)
                .unique_value_not_in(&Domain::new(i64::MAX, i64::MAX)),
            Some(i64::MAX - 1)
        );

        // 4. Two values remaining at the extreme upper edge.
        // Ensures the `return None` optimization fires safely at the
        // boundary.
        assert_eq!(
            Domain::new(i64::MAX - 1, i64::MAX).unique_value_not_in(&Domain::new(0, 0)),
            None
        );

        // 5. Exclude the entire representable integer space.
        assert_eq!(
            Domain::new(i64::MAX, i64::MAX).unique_value_not_in(&Domain::new(i64::MIN, i64::MAX)),
            None
        );

        // 6. Lower bound safety (INT64_MIN).
        // Ensures subtraction or logic doesn't underflow at the bottom boundary.
        assert_eq!(
            Domain::new(i64::MIN, i64::MIN + 5)
                .unique_value_not_in(&Domain::new(i64::MIN + 1, i64::MIN + 5)),
            Some(i64::MIN)
        );
    }

    #[test]
    fn domain_test_union_with_overflow_case() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::new(1, i64::MAX).union_with(&Domain::new(18, i64::MAX)),
            Domain::new(1, i64::MAX)
        );
    }

    #[test]
    fn domain_test_union_with_list_of_values_case() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            "[-1,0][5,6][8][10]",
            Domain::from_values(vec![0, 6, 8])
                .union_with(&Domain::from_values(vec![-1, 0, 5, 10]))
                .to_string()
        );
    }

    #[test]
    fn domain_test_complement_empty_cases() {
        // Arrange
        // Act
        // Assert
        assert_eq!(Domain::default().complement(), Domain::all_values());
        assert_eq!(Domain::all_values().complement(), Domain::default());
    }

    #[test]
    fn domain_test_complement_corner_cases() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::new(i64::MIN + 1, i64::MAX),
            Domain::from_values(vec![i64::MIN]).complement()
        );
        assert_eq!(
            Domain::new(i64::MIN + 1, i64::MAX).complement(),
            Domain::from_values(vec![i64::MIN])
        );
        assert_eq!(
            Domain::new(i64::MIN, i64::MAX - 1),
            Domain::from_values(vec![i64::MAX]).complement()
        );
        assert_eq!(
            Domain::new(i64::MIN, i64::MAX - 1).complement(),
            Domain::from_values(vec![i64::MAX])
        );
    }

    #[test]
    fn domain_test_complement_normal_cases() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::from_intervals(&intervals(&[(i64::MIN, -1), (1, i64::MAX)])),
            Domain::from_values(vec![0]).complement()
        );

        // The function is involutive.
        assert_eq!(
            Domain::from_intervals(&intervals(&[(i64::MIN, -3), (3, 3), (7, i64::MAX)])),
            Domain::from_intervals(&intervals(&[(-2, 2), (4, 6)])).complement()
        );
        assert_eq!(
            Domain::from_intervals(&intervals(&[(-2, 2), (4, 6)])),
            Domain::from_intervals(&intervals(&[(i64::MIN, -3), (3, 3), (7, i64::MAX)]))
                .complement()
        );
    }

    #[test]
    fn domain_test_negation_normal_cases() {
        // Arrange
        // Act
        // Assert
        assert_eq!(Domain::default().negation(), Domain::default());
        assert_eq!(
            Domain::from_intervals(&intervals(&[(-5, 0), (7, 7)])).negation(),
            Domain::from_intervals(&intervals(&[(-7, -7), (0, 5)]))
        );
    }

    #[test]
    fn domain_test_negation_corner_cases() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::all_values().negation(),
            Domain::new(-i64::MAX, i64::MAX)
        );
        assert_eq!(
            Domain::from_values(vec![i64::MIN]).negation(),
            Domain::default()
        );
        assert_eq!(
            Domain::new_value(i64::MAX).negation(),
            Domain::new_value(-i64::MAX)
        );
        assert_eq!(
            Domain::from_intervals(&intervals(&[(i64::MIN, i64::MIN), (-5, -2)])).negation(),
            Domain::new(2, 5)
        );
    }

    #[test]
    fn domain_test_continuous_multiplication_by_basic_cases() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::default().continuous_multiplication_by(3),
            Domain::default()
        );
        assert_eq!(
            Domain::new(5, 7).continuous_multiplication_by(3),
            Domain::new(15, 21)
        );
        assert_eq!(
            Domain::new(5, 7).continuous_multiplication_by(-2),
            Domain::new(-14, -10)
        );
        assert_eq!(
            Domain::all_values().continuous_multiplication_by(-2),
            Domain::new(-i64::MAX, i64::MAX)
        );
    }

    #[test]
    fn domain_test_continuous_multiplication_between_domains() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::default().continuous_multiplication_by_domain(&Domain::new(2, 3)),
            Domain::default()
        );
        assert_eq!(
            Domain::new(5, 7).continuous_multiplication_by_domain(&Domain::new(2, 3)),
            Domain::new(10, 21)
        );
        assert_eq!(
            Domain::new(5, 7).continuous_multiplication_by_domain(&Domain::new(-2, 5)),
            Domain::new(-14, 35)
        );
        assert_eq!(
            Domain::new(-5, 7).continuous_multiplication_by_domain(&Domain::new(-10, 5)),
            Domain::new(-70, 50)
        );
    }

    #[test]
    fn domain_test_division_by_basic_cases() {
        // Arrange
        // Act
        // Assert
        assert_eq!(Domain::default().division_by(3), Domain::default());
        assert_eq!(Domain::new(5, 7).division_by(3), Domain::new(1, 2));
        assert_eq!(Domain::new(5, 7).division_by(-2), Domain::new(-3, -2));
        assert_eq!(
            Domain::all_values().division_by(-2),
            Domain::new(-(i64::MAX / 2), -(i64::MIN / 2))
        );
    }

    #[test]
    fn domain_test_multiplication_by_basic_case() {
        // Arrange
        let mut exact = false;

        // Act
        // Assert
        assert_eq!(
            Domain::default().multiplication_by(3, Some(&mut exact)),
            Domain::default()
        );
        assert!(exact);

        assert_eq!(
            Domain::from_intervals(&intervals(&[(-2, 2), (4, 5)]))
                .multiplication_by(3, Some(&mut exact))
                .to_string(),
            "[-6][-3][0][3][6][12][15]"
        );
        assert!(exact);

        assert_eq!(
            Domain::new(-20000, 10000).multiplication_by(3, Some(&mut exact)),
            Domain::new(-60000, 30000)
        );
        assert!(!exact);

        assert_eq!(
            Domain::new(-20000, 10000).multiplication_by(-1, Some(&mut exact)),
            Domain::new(-10000, 20000)
        );
        assert!(exact);
    }

    #[test]
    fn domain_test_multiplication_by_zero() {
        // Arrange
        let mut exact = false;

        // Act
        // Assert
        assert_eq!(
            Domain::new(5, 7).multiplication_by(0, Some(&mut exact)),
            Domain::new(0, 0)
        );
        assert!(exact);
        assert_eq!(
            Domain::new(0, 0).multiplication_by(0, Some(&mut exact)),
            Domain::new(0, 0)
        );
        assert!(exact);
        assert_eq!(
            Domain::default().multiplication_by(0, Some(&mut exact)),
            Domain::default()
        );
        assert!(exact);
    }

    #[test]
    fn domain_test_multiplication_by_and_overflow() {
        // Arrange
        let mut exact = false;

        // Act
        // Assert
        // Interestingly kint64max = 7 * 7 * 73 * 127 * 337 * 92737 * 649657
        assert_eq!(
            Domain::new_value(i64::MAX / 7).multiplication_by(7, Some(&mut exact)),
            Domain::new_value(i64::MAX)
        );
        assert_eq!(
            Domain::new_value(i64::MAX / 337).multiplication_by(337, Some(&mut exact)),
            Domain::new_value(i64::MAX)
        );

        // And for kint64min it is just -2^63. But if we multiply by a negative
        // number, we will always loose kint64min due to the implementation. It is
        // probably not a big deal in our context.
        assert_eq!(
            Domain::new_value(i64::MIN / 8).multiplication_by(8, Some(&mut exact)),
            Domain::new_value(i64::MIN)
        );
        assert_eq!(
            Domain::new_value(-(i64::MIN / 8)).multiplication_by(-8, Some(&mut exact)),
            Domain::default()
        );

        // Depending on how we iteration on a domain value, it is easy to cause an
        // overflow. This used to fail because of the value++.
        assert_eq!(
            Domain::new_value(i64::MAX).multiplication_by(2, None),
            Domain::default()
        );
    }

    #[test]
    fn domain_test_addition_with_basic_cases() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::default().addition_with(&Domain::new(2, 3)),
            Domain::default()
        );
        assert_eq!(
            Domain::new(3, 4).addition_with(&Domain::default()),
            Domain::default()
        );
        assert_eq!(
            Domain::from_intervals(&intervals(&[(1, 2), (4, 4)])).addition_with(&Domain::new(0, 3)),
            Domain::new(1, 7)
        );
    }

    #[test]
    fn domain_test_addition_with_positive_overflow() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::new(-3, 4).addition_with(&Domain::new_value(i64::MAX)),
            Domain::new(i64::MAX - 3, i64::MAX)
        );
        assert_eq!(
            Domain::new_value(0).addition_with(&Domain::new_value(i64::MAX)),
            Domain::new_value(i64::MAX)
        );
        assert!(
            Domain::new_value(1)
                .addition_with(&Domain::new_value(i64::MAX))
                .is_empty()
        );
    }

    #[test]
    fn domain_test_addition_with_negative_overflow() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::new(-3, 4).addition_with(&Domain::new_value(i64::MIN)),
            Domain::new(i64::MIN, i64::MIN + 4)
        );
        assert_eq!(
            Domain::new_value(0).addition_with(&Domain::new_value(i64::MIN)),
            Domain::new_value(i64::MIN)
        );
        assert!(
            Domain::new_value(-1)
                .addition_with(&Domain::new_value(i64::MIN))
                .is_empty()
        );
    }

    #[test]
    fn domain_test_inverse_multiplication_by_empty_cases() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::default().inverse_multiplication_by(3),
            Domain::default()
        );
        assert_eq!(
            Domain::new(5, 7).inverse_multiplication_by(4),
            Domain::default()
        );
        assert_eq!(
            Domain::new(5, 7).inverse_multiplication_by(8),
            Domain::default()
        );
    }

    #[test]
    fn domain_test_inverse_multiplication_by_zero_coeff() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::new(-1, 0).inverse_multiplication_by(0),
            Domain::all_values()
        );
        assert_eq!(
            Domain::new(1, 7).inverse_multiplication_by(0),
            Domain::default()
        );
    }

    #[test]
    fn domain_test_inverse_multiplication_by_normal_cases() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::from_intervals(&intervals(&[(-5, -1), (1, 1), (3, 8)]))
                .inverse_multiplication_by(2),
            Domain::from_intervals(&intervals(&[(-2, -1), (2, 4)]))
        );
        assert_eq!(
            Domain::from_intervals(&intervals(&[(-5, -1), (1, 1), (3, 8)]))
                .inverse_multiplication_by(3),
            Domain::from_intervals(&intervals(&[(-1, -1), (1, 2)]))
        );
        assert_eq!(
            Domain::from_intervals(&intervals(&[(2, 2)])).inverse_multiplication_by(2),
            Domain::from_intervals(&intervals(&[(1, 1)]))
        );
        assert_eq!(
            Domain::from_intervals(&intervals(&[(2, 2), (4, 4), (6, 7)]))
                .inverse_multiplication_by(2),
            Domain::from_intervals(&intervals(&[(1, 3)]))
        );
    }

    #[test]
    fn domain_test_inverse_multiplication_by_negative_cases() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::new(-5, -1).inverse_multiplication_by(-2),
            Domain::new(1, 2)
        );
        assert_eq!(
            Domain::new(-5, 8).inverse_multiplication_by(-2),
            Domain::new(-4, 2)
        );
        assert_eq!(
            Domain::new(4, 8).inverse_multiplication_by(-2),
            Domain::new(-4, -2)
        );
    }

    #[test]
    fn domain_test_inverse_multiplication_by_no_overflow_cases() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::all_values().inverse_multiplication_by(1000),
            Domain::new(i64::MIN / 1000, i64::MAX / 1000)
        );
        assert_eq!(
            Domain::from_values(vec![i64::MAX]).inverse_multiplication_by(2),
            Domain::default()
        );
        assert_eq!(
            Domain::from_values(vec![i64::MIN]).inverse_multiplication_by(2),
            Domain::from_values(vec![i64::MIN / 2])
        );
    }

    #[test]
    fn domain_test_simplify_using_implied_domain() {
        // Arrange
        // Act
        // Assert
        assert!(
            Domain::new(-5, 7)
                .simplify_using_implied_domain(&Domain::default())
                .is_empty()
        );
        assert!(
            Domain::new(-5, 7)
                .simplify_using_implied_domain(&Domain::new(8, 9))
                .is_empty()
        );
        assert_eq!(
            Domain::from_intervals(&intervals(&[(-5, -1), (1, 1)])).simplify_using_implied_domain(
                &Domain::from_intervals(&intervals(&[(-5, -1), (1, 1)]))
            ),
            Domain::new(-5, 1)
        );
        assert_eq!(
            Domain::new(-10, 10).simplify_using_implied_domain(&Domain::from_intervals(
                &intervals(&[(-5, -1), (1, 1)])
            )),
            Domain::new(-5, 1)
        );
        assert_eq!(
            Domain::new(1, 2).simplify_using_implied_domain(&Domain::new(-10, 10)),
            Domain::new(1, 2)
        );
        assert_eq!(
            Domain::from_intervals(&intervals(&[(1, 2), (7, 9)])).simplify_using_implied_domain(
                &Domain::from_intervals(&intervals(&[(0, 4), (6, 10)]))
            ),
            Domain::from_intervals(&intervals(&[(1, 2), (7, 9)]))
        );
        assert_eq!(
            Domain::from_intervals(&intervals(&[(1, 2), (7, 9)])).simplify_using_implied_domain(
                &Domain::from_intervals(&intervals(&[(0, 2), (7, 10)]))
            ),
            Domain::new(1, 9)
        );
        assert_eq!(
            Domain::from_intervals(&intervals(&[(1, 2), (7, 9)])).simplify_using_implied_domain(
                &Domain::from_intervals(&intervals(&[(0, 2), (7, 8)]))
            ),
            Domain::new(1, 8)
        );
    }

    #[test]
    fn domain_test_sum_of_k_min_value_in_domain() {
        // Arrange
        let d = Domain::from_intervals(&intervals(&[(1, 3), (5, 5), (7, 9)]));

        // Act
        // Assert
        assert_eq!(0, sum_of_k_min_value_in_domain(&d, -1));
        assert_eq!(0, sum_of_k_min_value_in_domain(&d, 0));
        assert_eq!(1, sum_of_k_min_value_in_domain(&d, 1));
        assert_eq!(3, sum_of_k_min_value_in_domain(&d, 2));
        assert_eq!(11, sum_of_k_min_value_in_domain(&d, 4));
        assert_eq!(18, sum_of_k_min_value_in_domain(&d, 5));
        assert_eq!(35, sum_of_k_min_value_in_domain(&d, 7));
        assert_eq!(35, sum_of_k_min_value_in_domain(&d, 8));
    }

    #[test]
    fn domain_test_sum_of_k_max_value_in_domain() {
        // Arrange
        let d = Domain::from_intervals(&intervals(&[(1, 3), (5, 5), (7, 9)]));

        // Act
        // Assert
        assert_eq!(0, sum_of_k_max_value_in_domain(&d, -1));
        assert_eq!(0, sum_of_k_max_value_in_domain(&d, 0));
        assert_eq!(9, sum_of_k_max_value_in_domain(&d, 1));
        assert_eq!(17, sum_of_k_max_value_in_domain(&d, 2));
        assert_eq!(29, sum_of_k_max_value_in_domain(&d, 4));
        assert_eq!(32, sum_of_k_max_value_in_domain(&d, 5));
        assert_eq!(35, sum_of_k_max_value_in_domain(&d, 7));
        assert_eq!(35, sum_of_k_max_value_in_domain(&d, 8));
    }

    #[test]
    fn domain_test_smallest_value() {
        // Arrange
        // Act
        // Assert
        assert_eq!(0, Domain::new(-2, 3).smallest_value());
        assert_eq!(0, Domain::new(0, 3).smallest_value());
        assert_eq!(
            1,
            Domain::from_intervals(&intervals(&[(-2, -1), (1, 2)])).smallest_value()
        );
        assert_eq!(
            -1,
            Domain::from_intervals(&intervals(&[(-2, -1), (2, 2)])).smallest_value()
        );
    }

    #[test]
    fn domain_test_part_around_zero() {
        // Arrange
        // Act
        // Assert
        assert_eq!(Domain::new(-2, 3), Domain::new(-2, 3).part_around_zero());
        assert_eq!(Domain::default(), Domain::new(1, 3).part_around_zero());
        assert_eq!(
            Domain::new(0, 1),
            Domain::from_intervals(&intervals(&[(-2, -1), (2, 2)]))
                .complement()
                .part_around_zero()
        );
    }

    #[test]
    fn domain_test_closest_value() {
        // Arrange
        // Act
        // Assert
        assert_eq!(1, Domain::new(-2, 3).closest_value(1));
        assert_eq!(-2, Domain::new(-2, 3).closest_value(-2));
        assert_eq!(-2, Domain::new(-2, 3).closest_value(-10));
        assert_eq!(3, Domain::new(-2, 3).closest_value(7));
        assert_eq!(
            1,
            Domain::from_intervals(&intervals(&[(-2, -1), (1, 2)])).closest_value(0)
        );
    }

    #[test]
    fn domain_test_value_at_or_before() {
        // Arrange
        // Act
        // Assert
        assert_eq!(1, Domain::new(-2, 3).value_at_or_before(1));
        assert_eq!(3, Domain::new(-2, 3).value_at_or_before(7));
        assert_eq!(
            -1,
            Domain::from_intervals(&intervals(&[(-2, -1), (1, 2)])).value_at_or_before(0)
        );
        assert_eq!(
            1,
            Domain::from_intervals(&intervals(&[(-2, -1), (1, 2)])).value_at_or_before(1)
        );

        // Error = no change.
        assert_eq!(
            -3,
            Domain::from_intervals(&intervals(&[(-2, -1), (1, 2)])).value_at_or_before(-3)
        );
    }

    #[test]
    fn domain_test_value_at_or_after() {
        // Arrange
        // Act
        // Assert
        assert_eq!(1, Domain::new(-2, 3).value_at_or_after(1));
        assert_eq!(-2, Domain::new(-2, 3).value_at_or_after(-7));
        assert_eq!(
            1,
            Domain::from_intervals(&intervals(&[(-2, -1), (1, 2)])).value_at_or_after(0)
        );
        assert_eq!(
            1,
            Domain::from_intervals(&intervals(&[(-2, -1), (1, 2)])).value_at_or_after(1)
        );
        assert_eq!(
            -2,
            Domain::from_intervals(&intervals(&[(-2, -1), (1, 2)])).value_at_or_after(-3)
        );

        // Error = no change.
        assert_eq!(
            8,
            Domain::from_intervals(&intervals(&[(-2, -1), (1, 2)])).value_at_or_after(8)
        );
    }

    #[test]
    fn domain_test_positive_modulo_by_superset() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::default().positive_modulo_by_superset(&Domain::new(1, 3)),
            Domain::default()
        );
        assert_eq!(
            Domain::new(-5, 2).positive_modulo_by_superset(&Domain::new(1, 5)),
            Domain::new(-4, 2)
        );
        assert_eq!(
            Domain::new(2, 3).positive_modulo_by_superset(&Domain::new_value(7)),
            Domain::new(2, 3)
        );
    }

    #[test]
    fn domain_test_superset_when_modulo_is_not_fixed() {
        // Arrange
        // Complexity is <= 30 ^ 3 * 10 ^ 3 = 27 Millions.
        let var_range: i64 = 30;
        let mod_range: i64 = 10;
        for min in -var_range..=var_range {
            for max in min..=var_range {
                for mod_min in 1..mod_range {
                    for mod_max in mod_min..mod_range {
                        // Act
                        let result = Domain::new(min, max)
                            .positive_modulo_by_superset(&Domain::new(mod_min, mod_max));
                        let mut min_seen = mod_range;
                        let mut max_seen = -mod_range;
                        for v in min..=max {
                            for m in mod_min..=mod_max {
                                let seen = v % m;
                                min_seen = min_seen.min(seen);
                                max_seen = max_seen.max(seen);
                            }
                        }

                        // Assert
                        assert_ne!(min_seen, mod_range); // There is no empty domain here.

                        // We are correct, but not exact.
                        assert!(min_seen >= result.min());
                        assert!(max_seen <= result.max());
                    }
                }
            }
        }
    }

    #[test]
    fn domain_test_exact_when_modulo_is_fixed() {
        // Arrange
        let range: i64 = 50;
        for min in -range..=range {
            for max in min..=range {
                for modulo in 1..range {
                    // Act
                    let result = Domain::new(min, max)
                        .positive_modulo_by_superset(&Domain::new_value(modulo));
                    let mut min_seen = range;
                    let mut max_seen = -range;
                    for v in min..=max {
                        let seen = v % modulo;
                        min_seen = min_seen.min(seen);
                        max_seen = max_seen.max(seen);
                    }

                    // Assert
                    assert_ne!(min_seen, range); // There is no empty domain here.
                    assert_eq!(min_seen, result.min());
                    assert_eq!(max_seen, result.max());
                }
            }
        }
    }

    fn brute_force_check_division_by_superset(domain: &Domain, divisor: &Domain) {
        let superset = domain.division_by_superset(divisor);

        // Check that all values are inside the superset.
        let mut values: Vec<i64> = Vec::with_capacity((domain.size() * divisor.size()) as usize);
        for v in domain.values() {
            for d in divisor.values() {
                if d == 0 {
                    continue;
                }
                let mut res: i128 = i128::from(v) / i128::from(d);
                res = res.min(i128::from(i64::MAX));
                res = res.max(i128::from(i64::MIN));
                assert!(superset.contains(res as i64));
                values.push(res as i64);
            }
        }

        // Check that the min/max is reached.
        if values.is_empty() {
            assert!(superset.is_empty());
            return;
        }
        let min_value = *values.iter().min().unwrap();
        let max_value = *values.iter().max().unwrap();
        assert_eq!(min_value, superset.min());
        assert_eq!(max_value, superset.max());
    }

    #[test]
    fn domain_test_division_by_superset() {
        // Arrange
        // Act
        // Assert
        // Empty.
        assert_eq!(
            Domain::default().division_by_superset(&Domain::new(1, 3)),
            Domain::default()
        );
        assert_eq!(
            Domain::new(-5, 2).division_by_superset(&Domain::default()),
            Domain::default()
        );

        // ++/++
        assert_eq!(
            Domain::new(2, 5).division_by_superset(&Domain::new(1, 5)),
            Domain::new(0, 5)
        );
        assert_eq!(
            Domain::new(4, 7).division_by_superset(&Domain::new(2, 3)),
            Domain::new(1, 3)
        );

        // -+/++
        assert_eq!(
            Domain::new(-5, 2).division_by_superset(&Domain::new(1, 5)),
            Domain::new(-5, 2)
        );
        assert_eq!(
            Domain::new(-7, 4).division_by_superset(&Domain::new(2, 3)),
            Domain::new(-3, 2)
        );

        // ++/--
        assert_eq!(
            Domain::new(2, 5).division_by_superset(&Domain::new(-5, -1)),
            Domain::new(-5, 0)
        );
        assert_eq!(
            Domain::new(4, 7).division_by_superset(&Domain::new(-3, -2)),
            Domain::new(-3, -1)
        );

        // -+/--
        assert_eq!(
            Domain::new(-5, 2).division_by_superset(&Domain::new(-5, -1)),
            Domain::new(-2, 5)
        );
        assert_eq!(
            Domain::new(-7, 4).division_by_superset(&Domain::new(-3, -2)),
            Domain::new(-2, 3)
        );

        // ++/-+, with 0 in the divisor domain.
        assert_eq!(
            Domain::new(2, 5).division_by_superset(&Domain::new(-5, 1)),
            Domain::from_flat_span_of_intervals(&[-5, 0, 2, 5])
        );
        assert_eq!(
            Domain::new(4, 7).division_by_superset(&Domain::new(-3, 2)),
            Domain::from_flat_span_of_intervals(&[-7, -1, 2, 7])
        );

        // ++/-+, with 0 not in the divisor domain.
        assert_eq!(
            Domain::new(4, 7)
                .division_by_superset(&Domain::from_flat_span_of_intervals(&[-5, -2, 2, 3])),
            Domain::new(-3, 3)
        );

        // --/-0
        assert_eq!(
            Domain::new(-7, -4).division_by_superset(&Domain::new(-5, 0)),
            Domain::new(0, 7)
        );

        // --/0+
        assert_eq!(
            Domain::new(-7, -4).division_by_superset(&Domain::new(0, 5)),
            Domain::new(-7, 0)
        );

        // --/00
        assert_eq!(
            Domain::new(i64::MIN, i64::MIN).division_by_superset(&Domain::new(0, 0)),
            Domain::default()
        );

        // Edge case: INT_MIN/-1 overflows INT_MAX.
        assert_eq!(
            Domain::new(i64::MIN, i64::MIN).division_by_superset(&Domain::new(-1, -1)),
            Domain::new(i64::MAX, i64::MAX)
        );

        for a in -10..10 {
            for b in -10..10 {
                for c in -10..10 {
                    for d in -10..10 {
                        brute_force_check_division_by_superset(
                            &Domain::new(a, b),
                            &Domain::new(c, d),
                        );
                    }
                }
            }
        }
    }

    #[test]
    fn domain_test_square_super_set_test() {
        // Arrange
        // Act
        // Assert
        assert_eq!(
            Domain::new(-40, 70).square_superset(),
            Domain::from_values(vec![
                0, 1, 4, 9, 16, 25, 36, 49, 64, 81, 100, 121, 144, 169, 196, 225, 256, 289, 324,
                361, 400, 441, 484, 529, 576, 625, 676, 729, 784, 841, 900, 961, 1024, 1089, 1156,
                1225, 1296, 1369, 1444, 1521, 1600, 1681, 1764, 1849, 1936, 2025, 2116, 2209, 2304,
                2401, 2500, 2601, 2704, 2809, 2916, 3025, 3136, 3249, 3364, 3481, 3600, 3721, 3844,
                3969, 4096, 4225, 4356, 4489, 4624, 4761, 4900
            ])
        );
        assert_eq!(
            Domain::from_flat_intervals(&[5, 10, 100, 200]).square_superset(),
            Domain::from_flat_intervals(&[25, 100, 10000, 40000])
        );
        assert_eq!(
            Domain::from_flat_intervals(&[5, 10, 100, 1i64 << 35]).square_superset(),
            Domain::from_flat_intervals(&[25, 100, 10000, i64::MAX])
        );
        assert_eq!(
            Domain::from_values(vec![100, 1i64 << 35]).square_superset(),
            Domain::from_values(vec![10000, i64::MAX])
        );
    }

    fn naive_quadratic_superset(a: i128, b: i128, c: i128, d: i128, domain: &Domain) -> Domain {
        let mut values: Vec<i64> = Vec::with_capacity(domain.size() as usize);
        for value in domain.values() {
            let value = i128::from(value);
            let mut res: i128 = (a * value + b) * (c * value + d);
            res = res.min(i128::from(i64::MAX));
            res = res.max(i128::from(i64::MIN));
            values.push(res as i64);
        }
        Domain::from_values(values).relax_if_too_complex()
    }

    #[test]
    fn domain_test_quadratic_superset_test() {
        // Arrange
        let values: [[i64; 4]; 4] = [
            [0, 1, 4, 9],
            [1, 1, 1, -1],
            [-2, 2, -2, 2],
            [-50, -50, 50, 50],
        ];
        for start in -10..10 {
            for size in 0..10 {
                let domain = Domain::from_flat_intervals(&[100 * start, 100 * start + 100 * size]);
                for value in &values {
                    // Act
                    let expected = naive_quadratic_superset(
                        i128::from(value[0]),
                        i128::from(value[1]),
                        i128::from(value[2]),
                        i128::from(value[3]),
                        &domain,
                    );
                    let actual = domain.quadratic_superset(value[0], value[1], value[2], value[3]);

                    // Assert
                    assert_eq!(expected.min(), actual.min());
                    assert_eq!(expected.max(), actual.max());
                }
            }
        }

        for value in &values {
            // Act
            let int_max = i64::from(i32::MAX);
            let domain = Domain::from_flat_intervals(&[int_max - 200, int_max]);
            let expected = naive_quadratic_superset(
                i128::from(value[0]),
                i128::from(value[1]),
                i128::from(value[2]),
                i128::from(value[3]),
                &domain,
            );
            let actual = domain.quadratic_superset(value[0], value[1], value[2], value[3]);

            // Assert
            assert_eq!(expected.is_empty(), actual.is_empty());
            if actual.is_empty() {
                continue;
            }
            assert_eq!(expected.min(), actual.min());
            assert_eq!(expected.max(), actual.max());
        }
    }

    #[test]
    fn domain_test_multiple_int_max() {
        // Arrange
        let int_max = i64::MAX;

        // Act
        let with_int_max = Domain::from_values(vec![0, 3, 5, int_max, 2, int_max]);

        // Assert
        assert_eq!(with_int_max.size(), 5);
        assert_eq!(
            with_int_max,
            Domain::from_intervals(&intervals(&[(0, 0), (2, 3), (5, 5), (int_max, int_max)]))
        );
    }

    #[test]
    fn sorted_disjoint_interval_list_test_construction_from_vector() {
        // Arrange
        let mut domain_intervals: Vec<ClosedInterval> = Vec::new();
        for interval in &Domain::from_values(vec![1, 2, 3, 7, 8, -4]) {
            domain_intervals.push(*interval);
        }

        // Act
        let intervals = SortedDisjointIntervalList::new_from_intervals(&domain_intervals);

        // Assert
        assert_eq!("[-4][1,3][7,8]", intervals.debug_string());
    }

    #[test]
    fn sorted_disjoint_interval_list_test_simplification() {
        // Arrange
        // Act
        // Assert
        // Insert in the correct order.
        let s1: Vec<i64> = vec![0, 20];
        let e1: Vec<i64> = vec![10, 30];
        let i1 = SortedDisjointIntervalList::new(&s1, &e1);
        assert_eq!(i1.debug_string(), "[0,10][20,30]");

        // Reverse intervals.
        let s2: Vec<i64> = vec![20, 0];
        let e2: Vec<i64> = vec![30, 10];
        let mut i2 = SortedDisjointIntervalList::default();
        i2.insert_intervals(&s2, &e2);
        assert_eq!(i2.debug_string(), "[0,10][20,30]");

        let mut i2b = SortedDisjointIntervalList::default();
        i2b.insert_interval(20, 30);
        i2b.insert_interval(0, 10);
        assert_eq!(i2b.debug_string(), "[0,10][20,30]");

        // Overlapping intervals (first case).
        let s3: Vec<i64> = vec![0, 10, 50];
        let e3: Vec<i64> = vec![20, 30, 60];
        let i3 = SortedDisjointIntervalList::new(&s3, &e3);
        assert_eq!(i3.debug_string(), "[0,30][50,60]");

        // Overlapping intervals (second case).
        let s4: Vec<i64> = vec![50, 10, 0];
        let e4: Vec<i64> = vec![60, 30, 20];
        let mut i4 = SortedDisjointIntervalList::default();
        i4.insert_intervals(&s4, &e4);
        assert_eq!(i4.debug_string(), "[0,30][50,60]");
        assert_eq!(2, i4.num_intervals());
        let mut it = i4.begin();
        assert_eq!("[0,30]", i4.at(it).debug_string());
        it += 1;
        assert_eq!("[50,60]", i4.at(it).debug_string());

        // Adjacent intervals should be merged.
        let s5: Vec<i64> = vec![0, 15];
        let e5: Vec<i64> = vec![14, 30];
        let mut i5 = SortedDisjointIntervalList::default();
        i5.insert_intervals(&s5, &e5);
        assert_eq!(i5.debug_string(), "[0,30]");
        assert_eq!(1, i5.num_intervals());
        assert_eq!("[0,30]", i5.at(i5.begin()).debug_string());
    }

    #[test]
    fn sorted_disjoint_interval_list_test_simplification_int() {
        // Arrange
        // Act
        // Assert
        // Insert in the correct order.
        let s1: Vec<i32> = vec![0, 20];
        let e1: Vec<i32> = vec![10, 30];
        let i1 = SortedDisjointIntervalList::new_i32(&s1, &e1);
        assert_eq!(i1.debug_string(), "[0,10][20,30]");

        // Reverse intervals.
        let s2: Vec<i32> = vec![20, 0];
        let e2: Vec<i32> = vec![30, 10];
        let mut i2 = SortedDisjointIntervalList::default();
        i2.insert_intervals_i32(&s2, &e2);
        assert_eq!(i2.debug_string(), "[0,10][20,30]");

        // Overlapping intervals (first case).
        let s3: Vec<i32> = vec![0, 10, 50];
        let e3: Vec<i32> = vec![20, 30, 60];
        let i3 = SortedDisjointIntervalList::new_i32(&s3, &e3);
        assert_eq!(i3.debug_string(), "[0,30][50,60]");

        // Overlapping intervals (second case).
        let s4: Vec<i32> = vec![50, 10, 0];
        let e4: Vec<i32> = vec![60, 30, 20];
        let mut i4 = SortedDisjointIntervalList::default();
        i4.insert_intervals_i32(&s4, &e4);
        assert_eq!(i4.debug_string(), "[0,30][50,60]");
        assert_eq!(2, i4.num_intervals());
        let mut it = i4.begin();
        assert_eq!("[0,30]", i4.at(it).debug_string());
        it += 1;
        assert_eq!("[50,60]", i4.at(it).debug_string());
    }

    #[test]
    fn sorted_disjoint_interval_list_test_first_interval_greater_or_equal() {
        // Arrange
        let mut i = SortedDisjointIntervalList::default();
        i.insert_interval(10, 20);
        i.insert_interval(30, 40);
        i.insert_interval(50, 60);
        i.insert_interval(70, 80);
        assert_eq!(4, i.num_intervals());

        // Act
        // Assert
        let first = |value: i64| {
            i.at(i.first_interval_greater_or_equal(value))
                .debug_string()
        };
        assert_eq!("[10,20]", first(5));
        assert_eq!("[10,20]", first(10));
        assert_eq!("[10,20]", first(15));
        assert_eq!("[10,20]", first(20));
        assert_eq!("[30,40]", first(25));
        assert_eq!("[30,40]", first(30));
        assert_eq!("[30,40]", first(35));
        assert_eq!("[30,40]", first(40));
        assert_eq!("[50,60]", first(45));
        assert_eq!("[50,60]", first(50));
        assert_eq!("[50,60]", first(55));
        assert_eq!("[50,60]", first(60));
        assert_eq!("[70,80]", first(65));
        assert_eq!("[70,80]", first(70));
        assert_eq!("[70,80]", first(75));
        assert_eq!("[70,80]", first(80));
        assert!(i.first_interval_greater_or_equal(85) == i.end());
    }

    #[test]
    fn sorted_disjoint_interval_list_test_last_interval_less_or_equal() {
        // Arrange
        let mut i = SortedDisjointIntervalList::default();
        i.insert_interval(10, 20);
        i.insert_interval(30, 40);
        i.insert_interval(50, 60);
        i.insert_interval(70, 80);
        assert_eq!(4, i.num_intervals());

        // Act
        // Assert
        let last = |value: i64| i.at(i.last_interval_less_or_equal(value)).debug_string();
        assert!(i.last_interval_less_or_equal(5) == i.end());
        assert_eq!("[10,20]", last(10));
        assert_eq!("[10,20]", last(15));
        assert_eq!("[10,20]", last(20));
        assert_eq!("[10,20]", last(25));
        assert_eq!("[30,40]", last(30));
        assert_eq!("[30,40]", last(35));
        assert_eq!("[30,40]", last(40));
        assert_eq!("[30,40]", last(45));
        assert_eq!("[50,60]", last(50));
        assert_eq!("[50,60]", last(55));
        assert_eq!("[50,60]", last(60));
        assert_eq!("[50,60]", last(65));
        assert_eq!("[70,80]", last(70));
        assert_eq!("[70,80]", last(75));
        assert_eq!("[70,80]", last(80));
        assert_eq!("[70,80]", last(85));
    }

    #[test]
    fn sorted_disjoint_interval_list_test_iterator() {
        // Arrange
        let mut i = SortedDisjointIntervalList::default();
        i.insert_interval(10, 20);
        i.insert_interval(30, 40);
        i.insert_interval(50, 60);
        i.insert_interval(70, 80);

        // Act
        for (count, interval) in i.into_iter().enumerate() {
            // Assert
            let count = count as i64;
            assert_eq!(10 + 20 * count, interval.start);
            assert_eq!(20 + 20 * count, interval.end);
        }
    }

    #[test]
    fn sorted_disjoint_interval_list_test_last() {
        // Arrange
        let mut i = SortedDisjointIntervalList::default();

        // Act
        // Assert
        i.insert_interval(10, 20);
        assert_eq!(i.last().debug_string(), "[10,20]");
        i.insert_interval(4, 5);
        assert_eq!(i.last().debug_string(), "[10,20]");
        i.insert_interval(34, 38);
        assert_eq!(i.last().debug_string(), "[34,38]");
        i.insert_interval(21, 33);
        assert_eq!(i.last().debug_string(), "[10,38]");
    }

    #[test]
    fn sorted_disjoint_interval_list_test_empty() {
        // Arrange
        let list = SortedDisjointIntervalList::default();

        // Act
        // Assert
        assert_eq!(list.num_intervals(), 0);
        let mut num_intervals = 0;
        for _unused in &list {
            num_intervals += 1;
        }
        assert_eq!(num_intervals, 0);
        assert!(list.begin() == list.end());
        for i in [i64::MIN, 0, i64::MAX] {
            assert!(list.first_interval_greater_or_equal(i) == list.end(), "{i}");
            assert!(list.last_interval_less_or_equal(i) == list.end(), "{i}");
        }
        assert_eq!(list.debug_string(), "");
    }

    fn expect_interval_list_equal(list: &SortedDisjointIntervalList, starts: &[i64], ends: &[i64]) {
        assert_eq!(starts.len(), list.num_intervals() as usize);
        for (i, interval) in list.into_iter().enumerate() {
            assert_eq!(starts[i], interval.start);
            assert_eq!(ends[i], interval.end);
        }
    }

    #[test]
    fn sorted_disjoint_interval_list_test_complement_interval() {
        // Arrange
        let mut list = SortedDisjointIntervalList::default();
        list.insert_interval(10, 20);
        list.insert_interval(30, 40);

        // Act
        // Assert
        // Interval larger than list.
        expect_interval_list_equal(
            &list.build_complement_on_interval(0, 50),
            &[0, 21, 41],
            &[9, 29, 50],
        );
        // Interval start overlaps list.
        expect_interval_list_equal(
            &list.build_complement_on_interval(10, 50),
            &[21, 41],
            &[29, 50],
        );
        expect_interval_list_equal(
            &list.build_complement_on_interval(15, 50),
            &[21, 41],
            &[29, 50],
        );
        expect_interval_list_equal(
            &list.build_complement_on_interval(20, 50),
            &[21, 41],
            &[29, 50],
        );
        expect_interval_list_equal(
            &list.build_complement_on_interval(25, 50),
            &[25, 41],
            &[29, 50],
        );
        expect_interval_list_equal(
            &list.build_complement_on_interval(29, 50),
            &[29, 41],
            &[29, 50],
        );
        // Interval end overlaps list.
        expect_interval_list_equal(
            &list.build_complement_on_interval(0, 40),
            &[0, 21],
            &[9, 29],
        );
        expect_interval_list_equal(
            &list.build_complement_on_interval(0, 35),
            &[0, 21],
            &[9, 29],
        );
        expect_interval_list_equal(
            &list.build_complement_on_interval(0, 30),
            &[0, 21],
            &[9, 29],
        );
        expect_interval_list_equal(
            &list.build_complement_on_interval(0, 29),
            &[0, 21],
            &[9, 29],
        );
        expect_interval_list_equal(
            &list.build_complement_on_interval(0, 21),
            &[0, 21],
            &[9, 21],
        );
        // Interval does not overlap list.
        expect_interval_list_equal(&list.build_complement_on_interval(0, 9), &[0], &[9]);
        expect_interval_list_equal(&list.build_complement_on_interval(21, 29), &[21], &[29]);
        expect_interval_list_equal(&list.build_complement_on_interval(41, 50), &[41], &[50]);
        // Interval overlaps list intervals.
        expect_interval_list_equal(&list.build_complement_on_interval(10, 20), &[], &[]);
        expect_interval_list_equal(&list.build_complement_on_interval(30, 40), &[], &[]);
        expect_interval_list_equal(&list.build_complement_on_interval(15, 35), &[21], &[29]);
    }

    #[test]
    fn sorted_disjoint_interval_list_test_complement_interval_on_empty_list() {
        // Arrange
        let list = SortedDisjointIntervalList::default();

        // Act
        // Assert
        expect_interval_list_equal(&list.build_complement_on_interval(0, 50), &[0], &[50]);
    }

    #[test]
    fn sorted_disjoint_interval_list_test_randomized_stress_test() {
        // Arrange
        let mut random = ChaCha8Rng::seed_from_u64(12345);
        let mut list = SortedDisjointIntervalList::default();
        let k_special_values: [i64; 8] = [
            i64::MIN,
            i64::MIN + 1,
            i64::MIN + 2,
            i64::MIN + 3,
            i64::MAX,
            i64::MAX - 1,
            i64::MAX - 2,
            i64::MAX - 3,
        ];
        let k_normal_value_span: i64 = 10000;
        for num_inserts in [1, 2, 3, 4, 10, 100, 100000] {
            // NOTE(user): Timing on 2016-05-04: 2.4s in fastbuild, 4.9s in opt.
            let num_iters = ((if cfg!(debug_assertions) {
                100000
            } else {
                1000000
            }) / num_inserts)
                .max(1);
            for _iter in 0..num_iters {
                // Insert 'num_inserts' random small intervals.
                list.clear();
                for _i in 0..num_inserts {
                    let start = if bernoulli(&mut random, 1.0 / 4.0) {
                        k_special_values
                            [uniform(&mut random, 0, k_special_values.len() as i64) as usize]
                    } else {
                        uniform(&mut random, 0, k_normal_value_span)
                    };
                    let end = cap_add(start, uniform(&mut random, 0, 5));

                    // Act
                    let it = list.insert_interval(start, end);

                    // Assert
                    assert!(
                        list.at(it).start <= start && list.at(it).end >= end,
                        "Inserted [{start},{end}], got {}, list: {}",
                        list.at(it),
                        list.debug_string()
                    );
                }

                // Verify that the interval list makes sense.
                let mut num_intervals = 0;
                let mut prev: Option<&ClosedInterval> = None;
                for interval in &list {
                    num_intervals += 1;
                    assert!(interval.start <= interval.end);
                    if let Some(prev) = prev {
                        assert!(interval.start > prev.end + 1);
                    }
                    prev = Some(interval);
                }
                assert_eq!(num_intervals, list.num_intervals());
                assert!(num_intervals <= num_inserts);
                assert!(num_intervals >= 1);

                // Stress-test O(num_insert) lookup operations (to spend about the same
                // time as doing the insertions).
                for _i in 0..(2 * num_inserts + 10) {
                    let v = if bernoulli(&mut random, 1.0 / 2.0) {
                        k_special_values
                            [uniform(&mut random, 0, k_special_values.len() as i64) as usize]
                    } else {
                        uniform(&mut random, 0, k_normal_value_span) - 1
                    };
                    let mut it = list.first_interval_greater_or_equal(v);
                    if it == list.end() {
                        it -= 1;
                        assert!(list.at(it).end < v, "{v}, list: {}", list.debug_string());
                    } else if list.at(it).start <= v {
                        // The returned interval contains "v".
                        assert!(
                            list.at(it).end >= v,
                            "{v}, interval: {}, list: {}",
                            list.at(it),
                            list.debug_string()
                        );
                    } else {
                        // The returned interval is strictly above "v".  Check that the
                        // previous interval is strictly below v.
                        if it != list.begin() {
                            it -= 1;
                            assert!(
                                list.at(it).end < v,
                                "{v}, interval: {}, list: {}",
                                list.at(it),
                                list.debug_string()
                            );
                        }
                    }
                }
            }
        }
    }

    // The cases below are not in the upstream test file. The expected values are what the
    // optimised reference build returns for each input.

    #[test]
    fn smallest_value_compares_exact_magnitudes_at_i64_min() {
        // Arrange
        let cases = [
            (vec![i64::MIN, -5], -5),
            (vec![i64::MIN, -1], -1),
            (vec![i64::MIN, i64::MIN], i64::MIN),
            (vec![i64::MIN, -7, -3, -2], -2),
            (vec![i64::MIN, -7, 3, 9], 3),
            (vec![i64::MIN, -7, 7, 9], 7),
        ];
        for (flat_intervals, expected) in cases {
            let domain = Domain::from_flat_intervals(&flat_intervals);

            // Act
            let value = domain.smallest_value();

            // Assert
            assert_eq!(expected, value, "domain: {domain}");
        }
    }

    #[test]
    fn distance_wraps_on_overflow() {
        // Arrange
        let cases = [
            (Domain::new_value(0), i64::MIN, i64::MIN),
            (Domain::new_value(i64::MIN), i64::MAX, -1),
            (Domain::new_value(i64::MIN), 0, i64::MIN),
            (Domain::new_value(i64::MAX), i64::MIN, -1),
            (Domain::new_value(-1), i64::MAX, i64::MIN),
            (Domain::new_value(1), i64::MIN, i64::MIN + 1),
            (Domain::new_value(i64::MAX), -1, i64::MIN),
            (Domain::new(i64::MIN, 5), i64::MAX, i64::MAX - 5),
            (Domain::new(i64::MIN, i64::MIN), i64::MAX, -1),
        ];
        for (domain, value, expected) in cases {
            // Act
            let distance = domain.distance(value);

            // Assert
            assert_eq!(expected, distance, "domain: {domain}, value: {value}");
        }
    }

    #[test]
    fn multiplication_by_i64_min_drops_every_overflowing_value() {
        // Arrange
        let cases = [
            (Domain::new(1, 3), "[]"),
            (Domain::new(i64::MIN, i64::MIN), "[]"),
            (Domain::new_value(0), "[]"),
            (Domain::new(-1, 1), "[]"),
            (Domain::new(0, 5000), "[-9223372036854775807,0]"),
            (
                Domain::new(i64::MIN, i64::MAX),
                "[-9223372036854775807,9223372036854775807]",
            ),
        ];
        for (domain, expected) in cases {
            // Act
            let product = domain.multiplication_by(i64::MIN, None);

            // Assert
            assert_eq!(expected, product.to_string(), "domain: {domain}");
        }
    }

    #[test]
    fn continuous_multiplication_by_i64_min_caps_by_the_sign_of_the_value() {
        // Arrange
        let cases = [
            (
                Domain::new(1, 3),
                "[-9223372036854775807,9223372036854775807]",
            ),
            (
                Domain::new(-3, 3),
                "[-9223372036854775807,9223372036854775807]",
            ),
            (Domain::new(i64::MIN, i64::MIN), "[]"),
            (Domain::new(0, 5000), "[-9223372036854775807,0]"),
            (Domain::new(-5000, 0), "[0,9223372036854775807]"),
            (Domain::new(-5000, -1), "[]"),
            (Domain::new_value(-1), "[]"),
            (Domain::new_value(1), "[]"),
            (Domain::new_value(0), "[0]"),
            (Domain::new_value(2), "[-9223372036854775807]"),
            (Domain::new_value(-2), "[]"),
        ];
        for (domain, expected) in cases {
            // Act
            let product = domain.continuous_multiplication_by(i64::MIN);

            // Assert
            assert_eq!(expected, product.to_string(), "domain: {domain}");
        }
    }

    #[test]
    fn division_by_i64_min_divides_by_the_wrapped_absolute_value() {
        // Arrange
        let cases = [
            (Domain::new(5, 7), "[0]"),
            (Domain::new(i64::MIN, i64::MIN), "[-1]"),
            (Domain::new(-5, -3), "[0]"),
        ];
        for (domain, expected) in cases {
            // Act
            let quotient = domain.division_by(i64::MIN);

            // Assert
            assert_eq!(expected, quotient.to_string(), "domain: {domain}");
        }
    }

    #[test]
    fn inverse_multiplication_by_i64_min_divides_by_the_wrapped_absolute_value() {
        // Arrange
        let cases = [
            (Domain::new(i64::MIN, i64::MAX), "[]"),
            (Domain::new(i64::MIN, i64::MIN), "[-1]"),
            (Domain::new(-5, 7), "[0]"),
            (Domain::new(i64::MAX, i64::MAX), "[]"),
            (Domain::new(i64::MIN, -1), "[]"),
        ];
        for (domain, expected) in cases {
            // A debug build stops at the check on the positive coefficient, as upstream does.
            if cfg!(debug_assertions) {
                expect_death(
                    || {
                        let _ = domain.inverse_multiplication_by(i64::MIN);
                    },
                    "positive_coeff > 0",
                );
                continue;
            }

            // Act
            let quotient = domain.inverse_multiplication_by(i64::MIN);

            // Assert
            assert_eq!(expected, quotient.to_string(), "domain: {domain}");
        }
    }

    #[test]
    fn positive_modulo_by_superset_wraps_the_negation_of_i64_min() {
        // Arrange
        let cases = [
            (Domain::new(i64::MIN, -1), Domain::new_value(5), "[]"),
            (Domain::new(i64::MIN, -1), Domain::new_value(1), "[0]"),
            (Domain::new(i64::MIN, -1), Domain::new_value(i64::MAX), "[]"),
            (Domain::new(i64::MIN, -1), Domain::new(2, 9), "[]"),
            (Domain::new(i64::MIN, -5), Domain::new_value(7), "[]"),
            (
                Domain::new(i64::MIN + 1, -1),
                Domain::new_value(7),
                "[-6,0]",
            ),
        ];
        for (domain, modulo, expected) in cases {
            // Act
            let remainders = domain.positive_modulo_by_superset(&modulo);

            // Assert
            assert_eq!(
                expected,
                remainders.to_string(),
                "domain: {domain}, modulo: {modulo}"
            );
        }
    }

    #[test]
    fn positive_modulo_by_superset_of_i64_min_takes_the_remainder_as_unsigned() {
        // Arrange
        let cases = [(Domain::new_value(5), "[-3]"), (Domain::new(2, 9), "[]")];
        let domain = Domain::new(i64::MIN, i64::MIN);
        for (modulo, expected) in cases {
            // A debug build stops at the check that the minimum is positive, as upstream does.
            if cfg!(debug_assertions) {
                expect_death(
                    || {
                        let _ = domain.positive_modulo_by_superset(&modulo);
                    },
                    "min > 0",
                );
                continue;
            }

            // Act
            let remainders = domain.positive_modulo_by_superset(&modulo);

            // Assert
            assert_eq!(expected, remainders.to_string(), "modulo: {modulo}");
        }
    }

    #[test]
    fn quadratic_superset_wraps_the_point_after_the_parabola_minimum() {
        // Arrange
        let domain = Domain::new(i64::MIN, i64::MIN + 5000);

        // Act
        let result = domain.quadratic_superset(1, i64::MIN + 1, 1, i64::MIN);

        // Assert
        assert_eq!("[9223372036854775807]", result.to_string());
    }

    #[test]
    fn sum_of_k_min_value_in_domain_goes_past_i32_max_without_wrapping() {
        // Arrange
        let domain = Domain::new(2_147_483_645, 2_147_483_655);
        let cases = [(3, 6_442_450_938), (4, 8_589_934_586), (8, 17_179_869_188)];
        for (k, expected) in cases {
            // Act
            let sum = sum_of_k_min_value_in_domain(&domain, k);

            // Assert
            assert_eq!(expected, sum, "k: {k}");
        }
    }
}
