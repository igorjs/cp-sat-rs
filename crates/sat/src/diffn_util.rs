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
// Ported from ortools/sat/diffn_util.h at commit c931f5f7. Changed: translated from C++ to Rust.
// Ported from ortools/sat/diffn_util.cc at commit c931f5f7. Changed: translated from C++ to Rust.

use std::fmt;

use rand_chacha::ChaCha8Rng;

use crate::integer_base::{IntegerValue, cap_prod_i, cap_sub_i};

// Signed overflow in `area`, `size_x` and `size_y` is undefined behaviour
// upstream; here it panics.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Rectangle {
    pub x_min: IntegerValue,
    pub x_max: IntegerValue,
    pub y_min: IntegerValue,
    pub y_max: IntegerValue,
}

impl Rectangle {
    pub fn grow_to_include(&mut self, other: &Rectangle) {
        self.x_min = self.x_min.min(other.x_min);
        self.y_min = self.y_min.min(other.y_min);
        self.x_max = self.x_max.max(other.x_max);
        self.y_max = self.y_max.max(other.y_max);
    }

    pub fn area(&self) -> IntegerValue {
        self.size_x() * self.size_y()
    }

    pub fn cap_area(&self) -> IntegerValue {
        cap_prod_i(
            cap_sub_i(self.x_max, self.x_min),
            cap_sub_i(self.y_max, self.y_min),
        )
    }

    pub fn size_x(&self) -> IntegerValue {
        self.x_max - self.x_min
    }

    pub fn size_y(&self) -> IntegerValue {
        self.y_max - self.y_min
    }

    pub fn is_disjoint(&self, other: &Rectangle) -> bool {
        self.x_min >= other.x_max
            || other.x_min >= self.x_max
            || self.y_min >= other.y_max
            || other.y_min >= self.y_max
    }

    // The methods below are not meant to be used with zero-area rectangles.

    // Returns an empty rectangle if no intersection.
    pub fn intersect(&self, other: &Rectangle) -> Rectangle {
        let ret_x_min = self.x_min.max(other.x_min);
        let ret_y_min = self.y_min.max(other.y_min);
        let ret_x_max = self.x_max.min(other.x_max);
        let ret_y_max = self.y_max.min(other.y_max);

        if ret_x_min >= ret_x_max || ret_y_min >= ret_y_max {
            Rectangle::get_empty()
        } else {
            Rectangle {
                x_min: ret_x_min,
                x_max: ret_x_max,
                y_min: ret_y_min,
                y_max: ret_y_max,
            }
        }
    }

    pub fn intersect_area(&self, other: &Rectangle) -> IntegerValue {
        let ret_x_min = self.x_min.max(other.x_min);
        let ret_y_min = self.y_min.max(other.y_min);
        let ret_x_max = self.x_max.min(other.x_max);
        let ret_y_max = self.y_max.min(other.y_max);

        if ret_x_min >= ret_x_max || ret_y_min >= ret_y_max {
            IntegerValue::new(0)
        } else {
            (ret_x_max - ret_x_min) * (ret_y_max - ret_y_min)
        }
    }

    pub fn is_inside_of(&self, other: &Rectangle) -> bool {
        self.x_min >= other.x_min
            && self.x_max <= other.x_max
            && self.y_min >= other.y_min
            && self.y_max <= other.y_max
    }

    // Returns `this \ other` as a set of disjoint rectangles of non-empty area.
    // The resulting vector will have at most four elements.
    pub fn region_difference(&self, other: &Rectangle) -> Vec<Rectangle> {
        let intersect = self.intersect(other);
        if intersect.size_x() == 0 {
            return vec![*self];
        }

        //-------------------
        //|   |    4    |   |
        //|   |---------|   |
        //| 1 |  other  | 2 |
        //|   |---------|   |
        //|   |    3    |   |
        //-------------------
        let mut result: Vec<Rectangle> = Vec::with_capacity(4);
        if self.x_min < intersect.x_min {
            // Piece 1
            result.push(Rectangle {
                x_min: self.x_min,
                x_max: intersect.x_min,
                y_min: self.y_min,
                y_max: self.y_max,
            });
        }
        if self.x_max > intersect.x_max {
            // Piece 2
            result.push(Rectangle {
                x_min: intersect.x_max,
                x_max: self.x_max,
                y_min: self.y_min,
                y_max: self.y_max,
            });
        }
        if self.y_min < intersect.y_min {
            // Piece 3
            result.push(Rectangle {
                x_min: intersect.x_min,
                x_max: intersect.x_max,
                y_min: self.y_min,
                y_max: intersect.y_min,
            });
        }
        if self.y_max > intersect.y_max {
            // Piece 4
            result.push(Rectangle {
                x_min: intersect.x_min,
                x_max: intersect.x_max,
                y_min: intersect.y_max,
                y_max: self.y_max,
            });
        }

        result
    }

    pub fn get_empty() -> Rectangle {
        Rectangle {
            x_min: IntegerValue::new(0),
            x_max: IntegerValue::new(0),
            y_min: IntegerValue::new(0),
            y_max: IntegerValue::new(0),
        }
    }
}

impl fmt::Display for Rectangle {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "rectangle(x({}..{}), y({}..{}))",
            self.x_min.value(),
            self.x_max.value(),
            self.y_min.value(),
            self.y_max.value()
        )
    }
}

// Returns the L2 distance between the centers of the two rectangles.
pub fn center_to_center_l2_distance(_a: &Rectangle, _b: &Rectangle) -> f64 {
    unimplemented!("phase 2: sat/diffn_util.h")
}

pub fn center_to_center_l_infinity_distance(_a: &Rectangle, _b: &Rectangle) -> f64 {
    unimplemented!("phase 2: sat/diffn_util.h")
}

// Visible for testing. The algo is in O(n^4) so shouldn't be used directly.
// Returns true if there exists a bounding box with too much energy.
pub fn boxes_are_in_energy_conflict(
    _rectangles: &[Rectangle],
    _energies: &[IntegerValue],
    _boxes: &[i32],
    _conflict: Option<&mut Rectangle>,
) -> bool {
    unimplemented!("phase 2: sat/diffn_util.cc")
}

// An O(n^2) algorithm to analyze all the relevant X intervals and infer a
// threshold of the y size of a bounding box after which there is no point
// checking for energy overload.
//
// Returns false on conflict, and fills the bounding box that caused the
// conflict.
//
// If transpose is true, we analyze the relevant Y intervals instead.
pub fn analyze_intervals(
    _transpose: bool,
    _boxes: &[i32],
    _rectangles: &[Rectangle],
    _rectangle_energies: &[IntegerValue],
    _x_threshold: &mut IntegerValue,
    _y_threshold: &mut IntegerValue,
    _conflict: Option<&mut Rectangle>,
) -> bool {
    unimplemented!("phase 2: sat/diffn_util.cc")
}

// Removes boxes with a size above the thresholds. Also randomizes the order.
// Because we rely on various heuristics, this allows changing the order from
// one call to the next.
pub fn filter_boxes_and_randomize<'a>(
    _cached_rectangles: &[Rectangle],
    _boxes: &'a mut [i32],
    _threshold_x: IntegerValue,
    _threshold_y: IntegerValue,
    _random: &mut ChaCha8Rng,
) -> &'a mut [i32] {
    unimplemented!("phase 2: sat/diffn_util.cc")
}

// Given the total energy of all rectangles (sum of energies[box]) we know that
// any box with an area greater than that cannot participate in any "bounding
// box" conflict. As we remove this box, the total energy decreases, so we might
// remove more. This works in O(n log n).
pub fn filter_boxes_that_are_too_large<'a>(
    _cached_rectangles: &[Rectangle],
    _energies: &[IntegerValue],
    _boxes: &'a mut [i32],
) -> &'a mut [i32] {
    unimplemented!("phase 2: sat/diffn_util.cc")
}

// 1D counterpart of RectangleInRange::GetMinimumIntersectionArea.
// Finds the minimum possible overlap of an interval of size `size` that fits in
// [range_min, range_max] and a second interval [interval_min, interval_max].
pub fn smallest_1d_intersection(
    _range_min: IntegerValue,
    _range_max: IntegerValue,
    _size: IntegerValue,
    _interval_min: IntegerValue,
    _interval_max: IntegerValue,
) -> IntegerValue {
    unimplemented!("phase 2: sat/diffn_util.cc")
}

// For a given set of N rectangles in `rectangles`, there might be up to
// N*(N-1)/2 pairs of rectangles that intersect one another. If each of these
// pairs describes an arc and each rectangle describes a node, the rectangles
// and their intersections describe a graph. This function returns the full
// spanning forest for this graph (i.e., a spanning tree for each connected
// component). This function allows knowing if a set of rectangles has any
// intersection, find an example intersection for each rectangle that has one,
// or split the rectangles into connected components according to their
// intersections.
//
// The returned tuples are the arcs of the spanning forest represented by their
// indices in the input vector.
//
// This function works with degenerate rectangles (i.e., points or lines) and
// has the same semantics for overlap as Rectangle::IsDisjoint().
//
// Note: This function runs in O(N (log N)^2) time on the input size, which
// would be impossible to do if we were to return all the intersections, which
// can be quadratic in number.
pub fn find_partial_rectangle_intersections(_rectangles: &[Rectangle]) -> Vec<(i32, i32)> {
    unimplemented!("phase 2: sat/diffn_util.cc")
}

// This function is faster than FindPartialRectangleIntersections() if one
// only wants to know if there is at least one intersection. It is in
// O(N log N).
//
// The interval set is a sorted Vec here, where upstream has a btree_set, so
// an insert or a removal is O(N) and the worst case is O(N^2).
//
// IMPORTANT: this assumes rectangles are already sorted by their x_min and does
// not support degenerate rectangles with zero area.
//
// If a pair {i, j} is returned, we will have i < j, and no intersection in
// the subset of rectangles in [0, j).
pub fn find_one_intersection_if_present(rectangles: &[Rectangle]) -> Option<(i32, i32)> {
    find_one_intersection_if_present_impl(rectangles, None)
}

// Same as FindOneIntersectionIfPresent() but supports degenerate rectangles
// with zero area.
pub fn find_one_intersection_if_present_with_zero_area(
    rectangles: &[Rectangle],
) -> Option<(i32, i32)> {
    let mut extra_arcs: Vec<(i32, i32)> = Vec::new();
    let postprocessed = convert_to_rectangle32_with_non_zero_sizes(rectangles, &mut extra_arcs);
    if !extra_arcs.is_empty() {
        return Some(extra_arcs[0]);
    }
    let result =
        find_one_intersection_if_present_impl(&postprocessed.rectangles_sorted_by_x_min, None)?;
    Some((
        postprocessed.rectangles_sorted_by_x_min[result.0 as usize].index,
        postprocessed.rectangles_sorted_by_x_min[result.1 as usize].index,
    ))
}

#[derive(Clone, Copy, Debug, Default)]
struct Rectangle32 {
    x_min: i32,
    x_max: i32,
    y_min: i32,
    y_max: i32,
    index: i32,
}

impl Rectangle32 {
    fn is_inside_of(&self, other: &Rectangle32) -> bool {
        self.x_min >= other.x_min
            && self.x_max <= other.x_max
            && self.y_min >= other.y_min
            && self.y_max <= other.y_max
    }
}

#[derive(Default)]
struct PostProcessedResult {
    rectangles_sorted_by_x_min: Vec<Rectangle32>,
    // Only the partial intersection search reads it, and that function is not ported yet.
    #[allow(dead_code)]
    bounding_box: (i32, i32), // Always starting at (0,0).
}

// This function is a preprocessing function for algorithms that find overlaps
// between rectangles. It does the following:
//  - It converts the arbitrary int64_t coordinates into a small integer by
//    sorting the possible values and assigning them consecutive integers.
//  - It grows zero size intervals to make them size one. This simplifies
//    things considerably, since it is hard to reason about degenerate
//    rectangles in the general algorithm.
//
// Note that the last point needs to be done with care. Imagine the following
// example:
//  +----------+
//  |          |
//  |          +--------------+
//  |          |              |
//  |          |   p,q    r   |
//  |          +----*-----*-+-+
//  |          |            |
//  |          |            |
//  |          |            |
//  |          +------------+
//  |          |
//  |          |
//  +----------+
// Where p,q and r are points (i.e., boxes of size 0x0) and p and q have the
// same coordinates. We replace them by the following:
//  +----------+
//  |          |
//  |          +----------------------+
//  |          |                      |
//  |          |                      |
//  |          +----+-+---------------+
//  |          |    |p|
//  |          |    +-+-+
//  |          |      |q|
//  |          |      +-+     +-+
//  |          |              |r|
//  |          +--------------+-+---+
//  |          |                    |
//  |          |                    |
//  |          |                    |
//  |          +--------------------+
//  |          |
//  |          |
//  +----------+
//
// That is a pretty radical deformation of the original shape, but it retains
// the property of whether a pair of rectangles intersect or not.
//
// If this function detects a pair (r1, r2) so that r1 is fully inside r2,
// it adds the pair to the `fully_contained_pairs` vector and omits r1 from
// the returned vector. Note that this is a best-effort heuristic and will not
// detect all fully contained pairs. Moreover, it will return each rectangle
// that is fully contained by another rectangle only once, even if it is
// contained by multiple rectangles.
fn convert_to_rectangle32_with_non_zero_sizes(
    rectangles: &[Rectangle],
    fully_contained_pairs: &mut Vec<(i32, i32)>,
) -> PostProcessedResult {
    if rectangles.is_empty() {
        return PostProcessedResult::default();
    }

    #[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
    enum Event {
        End = 0,
        Point = 1,
        Begin = 2,
    }
    let mut x_events: Vec<(IntegerValue, Event, i32)> = Vec::with_capacity(rectangles.len() * 2);
    let mut y_events: Vec<(IntegerValue, Event, i32)> = Vec::with_capacity(rectangles.len() * 2);
    for (i, r) in rectangles.iter().enumerate() {
        let i = i as i32;
        debug_assert!(r.size_x() >= 0);
        debug_assert!(r.size_y() >= 0);
        if r.size_x() == 0 {
            x_events.push((r.x_min, Event::Point, i));
        } else {
            x_events.push((r.x_min, Event::Begin, i));
            x_events.push((r.x_max, Event::End, i));
        }
        if r.size_y() == 0 {
            y_events.push((r.y_min, Event::Point, i));
        } else {
            y_events.push((r.y_min, Event::Begin, i));
            y_events.push((r.y_max, Event::End, i));
        }
    }
    y_events.sort();

    let mut rectangles32: Vec<Rectangle32> = vec![Rectangle32::default(); rectangles.len()];
    let mut prev_y = IntegerValue::new(0);
    let mut prev_event = Event::End;
    let mut cur_index: i32 = -1;
    for &(y, event, index) in &y_events {
        if (prev_event != event && prev_event != Event::End)
            || prev_y != y
            || event == Event::Point
            || cur_index == -1
        {
            cur_index += 1;
        }

        let rectangle32 = &mut rectangles32[index as usize];
        match event {
            Event::Begin => {
                rectangle32.y_min = cur_index;
                rectangle32.index = index;
            }
            Event::End => {
                rectangle32.y_max = cur_index;
            }
            Event::Point => {
                rectangle32.y_min = cur_index;
                rectangle32.y_max = cur_index + 1;
                rectangle32.index = index;
            }
        }
        prev_event = event;
        prev_y = y;
    }
    let max_y_index = cur_index + 1;

    drop(y_events);

    x_events.sort();
    let mut prev_x = IntegerValue::new(0);
    prev_event = Event::End;
    cur_index = -1;
    for &(x, event, index) in &x_events {
        if (prev_event != event && prev_event != Event::End)
            || prev_x != x
            || event == Event::Point
            || cur_index == -1
        {
            cur_index += 1;
        }

        let rectangle32 = &mut rectangles32[index as usize];
        match event {
            Event::Begin => {
                rectangle32.x_min = cur_index;
            }
            Event::End => {
                rectangle32.x_max = cur_index;
            }
            Event::Point => {
                rectangle32.x_min = cur_index;
                rectangle32.x_max = cur_index + 1;
            }
        }
        prev_event = event;
        prev_x = x;
    }
    let max_x_index = cur_index + 1;

    let mut sorted_rectangles32: Vec<Rectangle32> = Vec::with_capacity(rectangles.len());

    let mut largest_box = Rectangle32::default();

    for &(_x, event, index) in &x_events {
        if event == Event::Begin || event == Event::Point {
            let rectangle = rectangles32[index as usize];

            if let Some(prev) = sorted_rectangles32.last().copied() {
                // Check if the current box is fully inside the previous box or the
                // largest box seen so far. If so, add it to the output and skip it
                // completely.
                if rectangle.is_inside_of(&prev) {
                    fully_contained_pairs.push((rectangle.index, prev.index));
                    continue;
                }

                if largest_box.index != prev.index && rectangle.is_inside_of(&largest_box) {
                    fully_contained_pairs.push((rectangle.index, largest_box.index));
                    continue;
                }

                // Update the largest box.
                let current_size =
                    (rectangle.y_max - rectangle.y_min).min(rectangle.x_max - rectangle.x_min);
                let remaining_x = largest_box.x_max - rectangle.x_min;

                let mut largest_remaining_size: i32 = -1;
                if remaining_x > 0 {
                    largest_remaining_size =
                        (largest_box.y_max - largest_box.y_min).min(remaining_x);
                }

                if current_size > largest_remaining_size {
                    largest_box = rectangle;
                }
            } else {
                // First element initialization
                largest_box = rectangle;
            }

            sorted_rectangles32.push(rectangle);
        }
    }

    PostProcessedResult {
        rectangles_sorted_by_x_min: sorted_rectangles32,
        bounding_box: (max_x_index, max_y_index),
    }
}

// The coordinates of a rectangle, so that one function serves `Rectangle` and
// `Rectangle32` as the C++ template over `RectangleT` does.
trait RectangleCoordinates {
    type Coordinate: Copy + Ord;

    fn x_min(&self) -> Self::Coordinate;
    fn x_max(&self) -> Self::Coordinate;
    fn y_min(&self) -> Self::Coordinate;
    fn y_max(&self) -> Self::Coordinate;
}

impl RectangleCoordinates for Rectangle {
    type Coordinate = IntegerValue;

    fn x_min(&self) -> IntegerValue {
        self.x_min
    }
    fn x_max(&self) -> IntegerValue {
        self.x_max
    }
    fn y_min(&self) -> IntegerValue {
        self.y_min
    }
    fn y_max(&self) -> IntegerValue {
        self.y_max
    }
}

impl RectangleCoordinates for Rectangle32 {
    type Coordinate = i32;

    fn x_min(&self) -> i32 {
        self.x_min
    }
    fn x_max(&self) -> i32 {
        self.x_max
    }
    fn y_min(&self) -> i32 {
        self.y_min
    }
    fn y_max(&self) -> i32 {
        self.y_max
    }
}

// If we happen to prove as a side-effect that some rectangles are disjoint from
// all the other rectangles, we add their indices to the
// `proven_disjoint_indices` vector.
fn find_one_intersection_if_present_impl<R: RectangleCoordinates>(
    rectangles: &[R],
    mut proven_disjoint_indices: Option<&mut Vec<i32>>,
) -> Option<(i32, i32)> {
    debug_assert!(
        rectangles
            .windows(2)
            .all(|pair| pair[0].x_min() <= pair[1].x_min())
    );

    // Set of boxes intersecting the sweep line. We only store y_min, other
    // coordinates can be accessed via rectangles[index].coordinate.
    struct Element<C> {
        index: i32,
        y_min: C,
    }

    // The btree_set of upstream is a vector sorted by y_min, and an iterator
    // is a position in it.
    let mut interval_set: Vec<Element<R::Coordinate>> = Vec::new();

    for (i, rectangle) in rectangles.iter().enumerate() {
        let i = i as i32;
        let x = rectangle.x_min();
        let y_min = rectangle.y_min();
        let y_max = rectangle.y_max();

        // TODO(user): We can handle that, but it requires some changes below.
        debug_assert!(y_min <= y_max);

        // Try to add this rectangle to the set, if there is an intersection, lazily
        // remove it if its x_max has already passed, otherwise report the
        // intersection.
        let mut it = interval_set.partition_point(|element| element.y_min < y_min);
        let inserted = it == interval_set.len() || y_min < interval_set[it].y_min;
        if inserted {
            interval_set.insert(it, Element { index: i, y_min });
        }
        if !inserted {
            let existing = interval_set[it].index;
            if rectangles[existing as usize].x_max() <= x {
                // We just replace if the rectangle at position i is stale.
                if let Some(indices) = proven_disjoint_indices.as_deref_mut() {
                    indices.push(existing);
                }
                interval_set[it].index = i;
            } else {
                // Intersection.
                return Some((existing, i));
            }
        } else {
            // If there was no element at position y_min, we need to test if the
            // interval before is stale or if it overlaps with the new one.
            if it > 0 {
                let it_before = it - 1;
                let index_before = interval_set[it_before].index;

                // Lazy erase stale entry.
                if rectangles[index_before as usize].x_max() <= x {
                    if let Some(indices) = proven_disjoint_indices.as_deref_mut() {
                        indices.push(index_before);
                    }
                    // `it` is the position of the element just after the one we erased.
                    interval_set.remove(it_before);
                    it = it_before;
                } else {
                    debug_assert!(interval_set[it_before].y_min <= y_min);
                    let y_max_before = rectangles[index_before as usize].y_max();
                    if y_max_before > y_min {
                        // Intersection.
                        return Some((index_before, i));
                    }
                }
            }
        }

        // We handled the part before, now we need to deal with the interval that
        // starts after y_min.
        it += 1;
        while it < interval_set.len() {
            let index = interval_set[it].index;
            // Lazy erase stale entry.
            if rectangles[index as usize].x_max() <= x {
                if let Some(indices) = proven_disjoint_indices.as_deref_mut() {
                    indices.push(index);
                }
                interval_set.remove(it);
                continue;
            }

            debug_assert!(y_min <= interval_set[it].y_min);
            if y_max > interval_set[it].y_min {
                // Intersection.
                return Some((index, i));
            }
            break;
        }
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::integer_base::K_MAX_INTEGER_VALUE;
    use rand_chacha::rand_core::{RngCore, SeedableRng};

    const K_RANDOM_SEED: u64 = 12345;

    // IntegerValue(v), as C++ converts an integer literal implicitly.
    fn iv(value: i64) -> IntegerValue {
        IntegerValue::new(value)
    }

    fn rect(x_min: i64, x_max: i64, y_min: i64, y_max: i64) -> Rectangle {
        Rectangle {
            x_min: iv(x_min),
            x_max: iv(x_max),
            y_min: iv(y_min),
            y_max: iv(y_max),
        }
    }

    // absl::Uniform(random, lo, hi): a value in [lo, hi), or lo for an empty range.
    fn uniform(random: &mut ChaCha8Rng, lo: i64, hi: i64) -> i64 {
        if hi <= lo {
            return lo;
        }
        lo + (random.next_u64() % (hi - lo) as u64) as i64
    }

    #[test]
    #[ignore = "phase 2: sat/diffn_util.h"]
    fn center_to_center_distance_test_basic_test() {
        let mut a = Rectangle::default();
        let mut b = Rectangle::default();
        a.x_min = iv(0);
        a.x_max = iv(2);
        a.y_min = iv(0);
        a.y_max = iv(2);
        b.x_min = iv(0);
        b.x_max = iv(10);
        b.y_min = iv(0);
        b.y_max = iv(10);

        // This is just the distance between (1,1) and (5,5) which should be sqrt(32).
        assert_eq!(center_to_center_l2_distance(&a, &b), 32.0_f64.sqrt());

        // Now this is 4.
        assert_eq!(center_to_center_l_infinity_distance(&a, &b), 4.0);
    }

    // PENDING(phase 2): TEST(GetOverlappingRectangleComponentsTest, Disconnected), needs sat/util.h
    // PENDING(phase 2): TEST(GetOverlappingRectangleComponentsTest, ComponentAndActive), needs sat/util.h

    #[test]
    #[ignore = "phase 2: sat/diffn_util.cc"]
    fn analyze_intervals_test_random() {
        // Generate a random set of intervals until the first conflict. We are in n^5!
        let mut random = ChaCha8Rng::seed_from_u64(K_RANDOM_SEED);
        let size: i64 = 20;
        let mut rectangles: Vec<Rectangle> = Vec::new();
        let mut energies: Vec<IntegerValue> = Vec::new();
        let mut boxes: Vec<i32> = Vec::new();
        for i in 0..40 {
            let mut box_ = Rectangle::default();
            box_.x_min = iv(uniform(&mut random, 0, size));
            box_.x_max = iv(uniform(&mut random, box_.x_min.value() + 1, size + 1));
            box_.y_min = iv(uniform(&mut random, 0, size));
            box_.y_max = iv(uniform(&mut random, box_.y_min.value() + 1, size + 1));
            rectangles.push(box_);
            boxes.push(i);
            let energy_x = iv(uniform(
                &mut random,
                1,
                (box_.x_max - box_.x_min + 1).value(),
            ));
            let energy_y = iv(uniform(
                &mut random,
                1,
                (box_.y_max - box_.y_min + 1).value(),
            ));
            energies.push(energy_x * energy_y);

            log::info!("{} {} energy:{}", i, box_, energies[energies.len() - 1]);
            let mut conflict = Rectangle::default();
            if !boxes_are_in_energy_conflict(&rectangles, &energies, &boxes, Some(&mut conflict)) {
                continue;
            }

            log::info!("Conflict! {conflict}");

            // Make sure whatever filter we do, we do not remove the conflict.
            let mut s: &mut [i32] = &mut boxes;
            let mut threshold_x = K_MAX_INTEGER_VALUE;
            let mut threshold_y = K_MAX_INTEGER_VALUE;
            for i in 0..4 {
                if !analyze_intervals(
                    i % 2 == 1,
                    s,
                    &rectangles,
                    &energies,
                    &mut threshold_x,
                    &mut threshold_y,
                    None,
                ) {
                    log::info!("Detected by analyse.");
                    return;
                }
                s = filter_boxes_and_randomize(
                    &rectangles,
                    s,
                    threshold_x,
                    threshold_y,
                    &mut random,
                );
                log::info!(
                    "Filtered size: {} x<={} y<={}",
                    s.len(),
                    threshold_x,
                    threshold_y
                );
                assert!(boxes_are_in_energy_conflict(
                    &rectangles,
                    &energies,
                    s,
                    None
                ));
            }

            break;
        }
    }

    #[test]
    #[ignore = "phase 2: sat/diffn_util.cc"]
    fn filter_boxes_that_are_too_large_test_empty() {
        let r: Vec<Rectangle> = Vec::new();
        let energies: Vec<IntegerValue> = Vec::new();
        let mut boxes: Vec<i32> = Vec::new();
        assert!(filter_boxes_that_are_too_large(&r, &energies, &mut boxes).is_empty());
    }

    #[test]
    #[ignore = "phase 2: sat/diffn_util.cc"]
    fn filter_boxes_that_are_too_large_test_basic_test() {
        let num_boxes: usize = 3;
        let mut r: Vec<Rectangle> = vec![Rectangle::default(); num_boxes];
        let energies: Vec<IntegerValue> = vec![iv(25); num_boxes];
        let mut boxes: Vec<i32> = vec![0, 1, 2];

        r[0] = rect(0, 5, 0, 5);
        r[1] = rect(0, 10, 0, 10);
        r[2] = rect(0, 6, 0, 6);

        assert_eq!(
            *filter_boxes_that_are_too_large(&r, &energies, &mut boxes),
            [0, 2]
        );
    }

    // PENDING(phase 2): TEST(ConstructOverlappingSetsTest, BasicTest), needs sat/util.h
    // PENDING(phase 2): TEST(ConstructOverlappingSetsTest, OneSet), needs sat/util.h
    // PENDING(phase 2): TEST(GetOverlappingIntervalComponentsTest, BasicTest), needs sat/diffn_util.h
    // PENDING(phase 2): TEST(GetOverlappingIntervalComponentsAndArticulationPointsTest, WithWeirdIndicesAndSomeCornerCases), needs sat/diffn_util.h
    // PENDING(phase 2): TEST(GetOverlappingIntervalComponentsTest, RandomizedStressTest), needs sat/diffn_util.h
    // PENDING(phase 2): TEST(GetIntervalArticulationPointsTest, RandomizedStressTest), needs sat/diffn_util.h
    // PENDING(phase 2): TEST(CapacityProfileTest, BasicApi), needs sat/diffn_util.h
    // PENDING(phase 2): TEST(CapacityProfileTest, ProfileWithMandatoryPart), needs sat/diffn_util.h

    fn naive_smallest_1d_intersection(
        range_min: IntegerValue,
        range_max: IntegerValue,
        size: IntegerValue,
        interval_min: IntegerValue,
        interval_max: IntegerValue,
    ) -> IntegerValue {
        let mut min_intersection = IntegerValue::MAX;
        let mut start = range_min;
        while start + size <= range_max {
            // Interval is [start, start + size]
            let intersection_start = start.max(interval_min);
            let intersection_end = (start + size).min(interval_max);
            let intersection_length = iv(0).max(intersection_end - intersection_start);
            min_intersection = min_intersection.min(intersection_length);
            start.pre_increment();
        }
        min_intersection
    }

    #[test]
    #[ignore = "phase 2: sat/diffn_util.cc"]
    fn smallest1_d_intersection_test_basic_test() {
        let mut random = ChaCha8Rng::seed_from_u64(K_RANDOM_SEED);
        let max_size: i64 = 20;
        const NUM_RUNS: i32 = 400;
        for _ in 0..NUM_RUNS {
            let range_min = iv(uniform(&mut random, 0, max_size - 1));
            let range_max = iv(uniform(&mut random, range_min.value() + 1, max_size));
            let size = iv(uniform(
                &mut random,
                1,
                range_max.value() - range_min.value(),
            ));

            let interval_min = iv(uniform(&mut random, 0, max_size - 1));
            let interval_max = iv(uniform(&mut random, interval_min.value() + 1, max_size));
            assert_eq!(
                naive_smallest_1d_intersection(
                    range_min,
                    range_max,
                    size,
                    interval_min,
                    interval_max
                ),
                smallest_1d_intersection(range_min, range_max, size, interval_min, interval_max)
            );
        }
    }

    #[test]
    fn rectangle_test_basic_test() {
        let r1 = rect(0, 2, 0, 2);
        let r2 = rect(1, 3, 1, 3);
        assert_eq!(r1.intersect(&r2), rect(1, 2, 1, 2));
    }

    #[test]
    fn rectangle_test_line_and_point_works_test() {
        let vertical_line = rect(1, 1, 0, 10);
        let r1 = rect(0, 3, 1, 3);
        assert!(!vertical_line.is_disjoint(&r1));

        let horizontal_line = rect(0, 10, 2, 2);
        assert!(!vertical_line.is_disjoint(&horizontal_line));

        let point = rect(1, 1, 2, 2);
        assert!(!r1.is_disjoint(&point));

        assert!(horizontal_line.is_disjoint(&point));
    }

    #[test]
    fn rectangle_test_random_region_difference_test() {
        let mut random = ChaCha8Rng::seed_from_u64(K_RANDOM_SEED);
        let size: i64 = 20;
        const NUM_RUNS: i32 = 400;
        for _ in 0..NUM_RUNS {
            let mut ret = [Rectangle::default(); 2];
            for r in &mut ret {
                r.x_min = iv(uniform(&mut random, 0, size - 1));
                r.x_max = r.x_min + iv(uniform(&mut random, 1, size - 1));
                r.y_min = iv(uniform(&mut random, 0, size - 1));
                r.y_max = r.y_min + iv(uniform(&mut random, 1, size - 1));
            }
            let set_diff = ret[0].region_difference(&ret[1]);
            assert_eq!(set_diff.is_empty(), ret[0].intersect(&ret[1]) == ret[0]);
            let mut diff_area = iv(0);
            for i in 0..set_diff.len() {
                for j in (i + 1)..set_diff.len() {
                    assert!(set_diff[i].is_disjoint(&set_diff[j]));
                }
                assert_ne!(set_diff[i].intersect(&ret[0]), Rectangle::get_empty());
                assert_eq!(set_diff[i].intersect(&ret[1]), Rectangle::get_empty());
                let area = set_diff[i].area();
                assert!(area > 0);
                diff_area += area;
            }
            assert_eq!(ret[0].intersect_area(&ret[1]) + diff_area, ret[0].area());
        }
    }

    // PENDING(phase 2): TEST(RectangleTest, RandomPavedRegionDifferenceTest), needs sat/2d_orthogonal_packing_testing.h
    // PENDING(phase 2): TEST(GetMinimumOverlapTest, BasicTest), needs sat/diffn_util.h
    // PENDING(phase 2): TEST(ProbingRectangleTest, BasicTest), needs sat/diffn_util.h
    // PENDING(phase 2): TEST(ProbingRectangleTest, Random), needs sat/diffn_util.h
    // PENDING(phase 2): TEST(ProbingRectangleTest, CounterExample), needs sat/diffn_util.h

    #[test]
    #[ignore = "phase 2: sat/diffn_util.cc"]
    fn find_partial_intersections_simple() {
        let r1 = rect(0, 2, 0, 2);
        let r2 = rect(1, 3, 1, 3);
        let r3 = rect(5, 8, 1, 3);
        let result = find_partial_rectangle_intersections(&[r1, r2, r3]);
        assert!(result == [(1, 0)] || result == [(0, 1)]);
    }

    // PENDING(phase 2): TEST(FindPartialIntersections, Random), needs sat/2d_orthogonal_packing_testing.h
    // PENDING(phase 2): TEST(FindEmptySpaces, Random), needs sat/2d_orthogonal_packing_testing.h
    // PENDING(phase 2): TEST(FindPairwiseRestrictionsTest, Random), needs sat/2d_orthogonal_packing_testing.h

    fn generate_dense_case(num_rectangles: i64) -> Vec<Rectangle> {
        let mut random = ChaCha8Rng::seed_from_u64(K_RANDOM_SEED);
        let mut rectangles: Vec<Rectangle> = Vec::new();
        for _ in 0..num_rectangles {
            let x_min = iv(uniform(&mut random, 0, num_rectangles));
            let y_min = iv(uniform(&mut random, 0, num_rectangles));
            let x_size = uniform(&mut random, 1, num_rectangles);
            let y_size = uniform(&mut random, 1, num_rectangles);
            rectangles.push(Rectangle {
                x_min,
                x_max: x_min + x_size,
                y_min,
                y_max: y_min + y_size,
            });
        }
        rectangles
    }

    #[test]
    #[ignore = "phase 2: sat/diffn_util.cc"]
    fn find_partial_intersections_dense_test_random() {
        let _result = find_partial_rectangle_intersections(&generate_dense_case(20));
    }

    // The upstream cases of the two intersection functions draw their input from
    // the generators of sat/2d_orthogonal_packing_testing.h. The cases below use
    // fixed rectangles. The expected pairs are what the reference build returns
    // for the same input.

    #[test]
    fn find_one_intersection_reports_two_overlapping_rectangles() {
        // Arrange
        let rectangles = [rect(0, 2, 0, 2), rect(1, 3, 1, 3), rect(5, 8, 1, 3)];

        // Act
        let one = find_one_intersection_if_present(&rectangles);
        let with_zero_area = find_one_intersection_if_present_with_zero_area(&rectangles);

        // Assert
        assert_eq!(one, Some((0, 1)));
        assert_eq!(with_zero_area, Some((0, 1)));
        assert_eq!(find_one_intersection_if_present(&rectangles[..1]), None);
        assert_eq!(
            find_one_intersection_if_present(&[rectangles[0], rectangles[2]]),
            None
        );
    }

    #[test]
    fn find_one_intersection_ignores_rectangles_that_only_touch_on_an_edge() {
        // Arrange
        let touching_on_x = [rect(0, 2, 0, 2), rect(2, 4, 0, 2)];
        let touching_on_y = [rect(0, 2, 0, 2), rect(0, 2, 2, 4)];

        // Act and Assert
        assert!(touching_on_x[0].is_disjoint(&touching_on_x[1]));
        assert_eq!(find_one_intersection_if_present(&touching_on_x), None);
        assert_eq!(
            find_one_intersection_if_present_with_zero_area(&touching_on_x),
            None
        );
        assert_eq!(find_one_intersection_if_present(&touching_on_y), None);
        assert_eq!(
            find_one_intersection_if_present_with_zero_area(&touching_on_y),
            None
        );
    }

    #[test]
    fn find_one_intersection_with_zero_area_finds_a_line_inside_a_rectangle() {
        // Arrange
        let big = rect(0, 10, 0, 10);
        let vertical_line = rect(5, 5, 2, 8);
        let outside_line = rect(10, 10, 2, 8);

        // Act
        let inside = find_one_intersection_if_present_with_zero_area(&[big, vertical_line]);
        let on_the_edge = find_one_intersection_if_present_with_zero_area(&[big, outside_line]);

        // Assert: the pair holds the input indices, the line first, as the reference gives.
        assert!(!vertical_line.is_disjoint(&big));
        assert_eq!(inside, Some((1, 0)));
        assert!(outside_line.is_disjoint(&big));
        assert_eq!(on_the_edge, None);
        assert_eq!(find_one_intersection_if_present_with_zero_area(&[]), None);
    }
}
