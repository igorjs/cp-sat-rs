//! The phase of each upstream file. A file that is not in the table has no phase.

/// The seven phases of the port, numbered from 0.
pub const PHASE_COUNT: u32 = 7;

const PHASE_ZERO_FILES: [&str; 24] = [
    "base/hash.cc",
    "base/hash.h",
    "port/proto_utils.cc",
    "port/proto_utils.h",
    "sat/cp_model_checker.cc",
    "sat/cp_model_checker.h",
    "sat/cp_model_solver.h",
    "sat/cp_model_test_utils.cc",
    "sat/cp_model_test_utils.h",
    "sat/cp_model_utils.cc",
    "sat/cp_model_utils.h",
    "sat/diffn_util.cc",
    "sat/diffn_util.h",
    "sat/integer_base.h",
    "sat/model.h",
    "sat/primary_variables.cc",
    "sat/primary_variables.h",
    "sat/sat_literal.h",
    "util/bitset.cc",
    "util/bitset.h",
    "util/saturated_arithmetic.h",
    "util/sorted_interval_list.cc",
    "util/sorted_interval_list.h",
    "util/strong_integers.h",
];

/// Every file the table names, so that the map has a row for it.
pub fn phased_files() -> impl Iterator<Item = &'static str> {
    PHASE_ZERO_FILES.into_iter()
}

pub fn phase_of(path: &str) -> Option<u32> {
    PHASE_ZERO_FILES.contains(&path).then_some(0)
}
