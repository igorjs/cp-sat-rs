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
// Ported from ortools/base/hash.h and ortools/base/hash.cc at commit c931f5f7. Changed: translated from C++ to Rust.

/// The 64 bit fast-hash of `buf`. The C++ reads each whole word of the buffer in the byte
/// order of the host, and this port reads little endian, the order of every supported host.
pub fn fasthash64(buf: &[u8], seed: u64) -> u64 {
    let m: u64 = 0x880355f21e6d1965;
    let (words, tail) = buf.as_chunks::<8>();
    let mut h: u64 = seed ^ (buf.len() as u64).wrapping_mul(m);

    for word in words {
        let v = u64::from_le_bytes(*word);
        h ^= mix_internal(v);
        h = h.wrapping_mul(m);
    }

    let mut v: u64 = 0;

    // The C++ switch falls through from the last byte of the tail down to the first.
    for (index, byte) in tail.iter().enumerate() {
        v ^= (*byte as u64) << (8 * index);
    }
    if !tail.is_empty() {
        h ^= mix_internal(v);
        h = h.wrapping_mul(m);
    }

    mix_internal(h)
}

// 64 bit version.
#[inline]
pub fn mix(a: &mut u64, b: &mut u64, c: &mut u64) {
    *a = a.wrapping_sub(*b);
    *a = a.wrapping_sub(*c);
    *a ^= *c >> 43;
    *b = b.wrapping_sub(*c);
    *b = b.wrapping_sub(*a);
    *b ^= *a << 9;
    *c = c.wrapping_sub(*a);
    *c = c.wrapping_sub(*b);
    *c ^= *b >> 8;
    *a = a.wrapping_sub(*b);
    *a = a.wrapping_sub(*c);
    *a ^= *c >> 38;
    *b = b.wrapping_sub(*c);
    *b = b.wrapping_sub(*a);
    *b ^= *a << 23;
    *c = c.wrapping_sub(*a);
    *c = c.wrapping_sub(*b);
    *c ^= *b >> 5;
    *a = a.wrapping_sub(*b);
    *a = a.wrapping_sub(*c);
    *a ^= *c >> 35;
    *b = b.wrapping_sub(*c);
    *b = b.wrapping_sub(*a);
    *b ^= *a << 49;
    *c = c.wrapping_sub(*a);
    *c = c.wrapping_sub(*b);
    *c ^= *b >> 11;
    *a = a.wrapping_sub(*b);
    *a = a.wrapping_sub(*c);
    *a ^= *c >> 12;
    *b = b.wrapping_sub(*c);
    *b = b.wrapping_sub(*a);
    *b ^= *a << 18;
    *c = c.wrapping_sub(*a);
    *c = c.wrapping_sub(*b);
    *c ^= *b >> 22;
}

/// `util_hash::Hash(num, c)`.
pub fn hash(mut num: u64, mut c: u64) -> u64 {
    let mut b: u64 = 0xe08c1d668b756f82; // More of the golden ratio.
    mix(&mut num, &mut b, &mut c);
    c
}

/// `util_hash::Hash(a, b, c)`.
pub fn hash_with_b(mut a: u64, mut b: u64, mut c: u64) -> u64 {
    mix(&mut a, &mut b, &mut c);
    c
}

// Compression function for Merkle-Damgard construction.
// This function is generated using the framework provided.
#[inline]
fn mix_internal(mut h: u64) -> u64 {
    h ^= h >> 23;
    h = h.wrapping_mul(0x2127599bf4325c37);
    h ^= h >> 47;
    h
}

#[cfg(test)]
mod tests {
    use super::*;
    use cpsat_testutil::fixture;

    const FIXTURE: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/tests/fixtures/fasthash64.txt");
    const CASE_COUNT: usize = 69;

    #[test]
    fn fasthash64_equals_the_hash_of_the_reference_for_every_case() {
        // Arrange
        let cases = fixture::read(FIXTURE);

        for case in &cases {
            let bytes = fixture::from_hex(case.single("bytes"));
            let seed: u64 = case.single("seed").parse().unwrap();
            let expected: u64 = case.single("hash").parse().unwrap();

            // Act
            let hash = fasthash64(&bytes, seed);

            // Assert
            assert_eq!(hash, expected, "{} bytes, seed {seed}", bytes.len());
        }
        assert_eq!(cases.len(), CASE_COUNT);
    }
}
