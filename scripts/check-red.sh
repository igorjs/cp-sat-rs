#!/usr/bin/env bash
# Proves that every ignored test is red because of a stub and for no other reason.
# Run it from the workspace root. It runs every ignored test, without stopping at the
# first failing test binary, and compares the tests that ran with the tests that
# `cargo test -- --ignored --list` names. The script fails unless every listed test ran
# and failed with a message that starts with "not implemented: phase".
set -euo pipefail

readonly STUB_MESSAGE_PREFIX="not implemented: phase"

work_dir="$(mktemp -d)"
trap 'rm -rf "$work_dir"' EXIT

# Reads cargo output on stdin. The "Running" lines of cargo name the test binary, so two
# binaries may hold tests with the same name. Prints one "<binary>/<test>" per listed
# test (mode "list"), or one "<binary>/<test><TAB><result><TAB><message>" per test that
# ran (mode "run"). Doc tests are left out.
parse_cargo_output() {
  awk -v mode="$1" '
    function binary_name(line,    path, n, parts) {
      sub(/^.*\(/, "", line)
      sub(/\)[^)]*$/, "", line)
      n = split(line, parts, "/")
      path = parts[n]
      sub(/-[0-9a-f]+$/, "", path)
      return path
    }
    /^ +Running / { binary = binary_name($0); test = ""; next }
    /^ +Doc-tests / { binary = ""; test = ""; next }
    binary == "" { next }
    mode == "list" && /: test$/ {
      name = $0
      sub(/: test$/, "", name)
      print binary "/" name
      next
    }
    mode == "run" && /^test .* \.\.\. / {
      name = $0
      sub(/^test /, "", name)
      result = name
      sub(/^.* \.\.\. /, "", result)
      sub(/ \.\.\. .*$/, "", name)
      status[binary "/" name] = result
      order[++count] = binary "/" name
      next
    }
    mode == "run" && /^---- .* stdout ----$/ {
      test = $0
      sub(/^---- /, "", test)
      sub(/ stdout ----$/, "", test)
      test = binary "/" test
      want_message = 0
      next
    }
    mode == "run" && test != "" && /panicked at / { want_message = 1; next }
    mode == "run" && test != "" && want_message {
      message[test] = $0
      want_message = 0
      next
    }
    END {
      if (mode == "run") {
        for (i = 1; i <= count; i++) {
          key = order[i]
          printf "%s\t%s\t%s\n", key, status[key], message[key]
        }
      }
    }
  '
}

cargo test --workspace -- --ignored --list 2>&1 | parse_cargo_output list | sort >"$work_dir/listed"

# Cargo exits non-zero when a test fails, which is the expected outcome here. A build
# error shows up as listed tests that did not run.
cargo test --workspace --no-fail-fast -- --ignored 2>&1 | parse_cargo_output run >"$work_dir/ran" || true

failures=0
total=0
while IFS= read -r test_name; do
  [ -n "$test_name" ] || continue
  total=$((total + 1))
  line="$(awk -F '\t' -v key="$test_name" '$1 == key { print; exit }' "$work_dir/ran")"
  if [ -z "$line" ]; then
    echo "check-red: ignored test did not run: ${test_name}" >&2
    failures=$((failures + 1))
    continue
  fi
  result="$(printf '%s' "$line" | cut -f 2)"
  message="$(printf '%s' "$line" | cut -f 3-)"
  if [ "$result" != "FAILED" ]; then
    echo "check-red: ignored test is not red, it ${result}: ${test_name}" >&2
    failures=$((failures + 1))
  elif [ "${message#"$STUB_MESSAGE_PREFIX"}" = "$message" ]; then
    echo "check-red: ignored test fails for another reason than a stub: ${test_name}: ${message}" >&2
    failures=$((failures + 1))
  fi
done <"$work_dir/listed"

if [ "$failures" -ne 0 ]; then
  echo "check-red: ${failures} of ${total} ignored tests are not red because of a stub" >&2
  exit 1
fi
echo "check-red: ${total} ignored tests, all red because of a stub"
