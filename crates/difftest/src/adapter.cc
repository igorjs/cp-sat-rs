#include "adapter.h"

#include <climits>
#include <memory>
#include <stdexcept>
#include <string>

#include "google/protobuf/descriptor.h"
#include "google/protobuf/message.h"
#include "ortools/base/hash.h"
#include "ortools/port/proto_utils.h"
#include "ortools/sat/cp_model.pb.h"
#include "ortools/sat/cp_model_checker.h"
#include "ortools/sat/cp_model_solver.h"
#include "ortools/sat/sat_parameters.pb.h"
#include "ortools/util/parse_proto.h"

namespace cpsat_difftest {
namespace {

namespace sat = operations_research::sat;

// ParseFromArray takes an int, so a larger input would wrap to a wrong size.
void ParseInto(google::protobuf::MessageLite& message,
               rust::Slice<const uint8_t> bytes, const std::string& what) {
  if (bytes.size() > static_cast<size_t>(INT_MAX)) {
    throw std::invalid_argument("the bytes for a " + what +
                                " are larger than 2 GiB");
  }
  if (!message.ParseFromArray(bytes.data(), static_cast<int>(bytes.size()))) {
    throw std::invalid_argument("the bytes are not a valid " + what);
  }
}

template <typename Proto>
Proto ParseBytes(rust::Slice<const uint8_t> bytes, const char* what) {
  Proto message;
  ParseInto(message, bytes, what);
  return message;
}

sat::CpModelProto ParseValidModel(rust::Slice<const uint8_t> bytes) {
  auto model = ParseBytes<sat::CpModelProto>(bytes, "CpModelProto");
  const std::string error = sat::ValidateCpModel(model);
  if (!error.empty()) {
    throw std::invalid_argument("the model is not valid: " + error);
  }
  return model;
}

std::unique_ptr<google::protobuf::Message> NewMessage(rust::Str name) {
  const std::string full_name(name.data(), name.size());
  const google::protobuf::Descriptor* descriptor =
      google::protobuf::DescriptorPool::generated_pool()
          ->FindMessageTypeByName(full_name);
  if (descriptor == nullptr) {
    throw std::invalid_argument("unknown message type " + full_name);
  }
  return std::unique_ptr<google::protobuf::Message>(
      google::protobuf::MessageFactory::generated_factory()
          ->GetPrototype(descriptor)
          ->New());
}

rust::Vec<uint8_t> ToVec(const std::string& bytes) {
  rust::Vec<uint8_t> out;
  out.reserve(bytes.size());
  for (const char byte : bytes) out.push_back(static_cast<uint8_t>(byte));
  return out;
}

}  // namespace

rust::Vec<uint8_t> reference_solve(rust::Slice<const uint8_t> model_bytes,
                                   rust::Slice<const uint8_t> parameter_bytes) {
  const auto model = ParseBytes<sat::CpModelProto>(model_bytes, "CpModelProto");
  const auto parameters =
      ParseBytes<sat::SatParameters>(parameter_bytes, "SatParameters");
  return ToVec(sat::SolveWithParameters(model, parameters).SerializeAsString());
}

rust::Vec<uint8_t> reference_parse_text(rust::Str message_name,
                                        rust::Str text) {
  const auto message = NewMessage(message_name);
  std::string error;
  if (!operations_research::ParseTextProtoForFlag(
          absl::string_view(text.data(), text.size()), message.get(),
          &error)) {
    throw std::invalid_argument(error);
  }
  return ToVec(message->SerializeAsString());
}

rust::String reference_debug_string(rust::Str message_name,
                                    rust::Slice<const uint8_t> bytes,
                                    bool short_form, int32_t max_chars,
                                    int32_t max_lines) {
  const auto message = NewMessage(message_name);
  ParseInto(*message, bytes, std::string(message_name));
  if (short_form) {
    return operations_research::ProtobufShortDebugString(*message);
  }
  return operations_research::ProtobufDebugString(*message, max_chars,
                                                  max_lines);
}

rust::String reference_validate(rust::Slice<const uint8_t> model_bytes) {
  const auto model = ParseBytes<sat::CpModelProto>(model_bytes, "CpModelProto");
  return sat::ValidateCpModel(model);
}

rust::String reference_validate_input(
    rust::Slice<const uint8_t> parameter_bytes,
    rust::Slice<const uint8_t> model_bytes) {
  const auto parameters =
      ParseBytes<sat::SatParameters>(parameter_bytes, "SatParameters");
  const auto model = ParseBytes<sat::CpModelProto>(model_bytes, "CpModelProto");
  return sat::ValidateInputCpModel(parameters, model);
}

bool reference_solution_is_feasible(rust::Slice<const uint8_t> model_bytes,
                                    rust::Slice<const int64_t> values) {
  const auto model = ParseValidModel(model_bytes);
  if (values.size() != static_cast<size_t>(model.variables_size())) {
    throw std::invalid_argument(
        "the number of values differs from the number of variables");
  }
  return sat::SolutionIsFeasible(
      model, absl::Span<const int64_t>(values.data(), values.size()));
}

rust::String reference_version() { return sat::CpSatSolverVersion(); }

uint64_t reference_fasthash64(rust::Slice<const uint8_t> bytes, uint64_t seed) {
  return operations_research::fasthash64(bytes.data(), bytes.size(), seed);
}

}  // namespace cpsat_difftest
