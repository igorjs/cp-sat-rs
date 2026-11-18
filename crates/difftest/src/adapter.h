// Calls into the reference solver for the Rust bridge in lib.rs.
#pragma once

#include <cstdint>

#include "rust/cxx.h"

namespace cpsat_difftest {

rust::Vec<uint8_t> reference_solve(rust::Slice<const uint8_t> model_bytes,
                                   rust::Slice<const uint8_t> parameter_bytes);

rust::Vec<uint8_t> reference_parse_text(rust::Str message_name,
                                        rust::Str text);

rust::String reference_debug_string(rust::Str message_name,
                                    rust::Slice<const uint8_t> bytes,
                                    bool short_form, int32_t max_chars,
                                    int32_t max_lines);

rust::String reference_validate(rust::Slice<const uint8_t> model_bytes);

rust::String reference_validate_input(
    rust::Slice<const uint8_t> parameter_bytes,
    rust::Slice<const uint8_t> model_bytes);

bool reference_solution_is_feasible(rust::Slice<const uint8_t> model_bytes,
                                    rust::Slice<const int64_t> values);

rust::String reference_version();

uint64_t reference_fasthash64(rust::Slice<const uint8_t> bytes, uint64_t seed);

}  // namespace cpsat_difftest
