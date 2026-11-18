#!/usr/bin/env bash
# Builds the reference solver: OR-Tools at the commit pinned in upstream.lock,
# from the source scripts/fetch-upstream.sh checked, as a shared C++ library
# with CP-SAT and without the other solvers. Run scripts/fetch-upstream.sh first.
#
# This build is expensive: it compiles OR-Tools and its dependencies from
# source. It does nothing when the prefix already holds this commit.
#
# Needs CMake, a C++20 compiler and the bzip2 development files.
#
# Environment:
#   UPSTREAM_LOCK        lock file to read (default: upstream.lock in the repo root)
#   UPSTREAM_DIR         checked source (default: upstream/ in the repo root)
#   REFERENCE_PREFIX     install directory
#                        (default: ~/.cache/cp-sat-rs/reference/<commit>)
#   REFERENCE_BUILD_DIR  CMake build directory
#                        (default: ~/.cache/cp-sat-rs/build/<commit>)
#   REFERENCE_JOBS       parallel compile jobs (default: 4)
set -euo pipefail

readonly DEFAULT_JOBS=4
readonly STAMP_NAME=.reference-commit
readonly CACHE_ROOT="$HOME/.cache/cp-sat-rs"

# CP-SAT is part of the core library, which has no switch. Every wrapper and
# optional component is off, and so is every other solver that needs a library.
# BOP and GLOP stay on because upstream does not support turning them off. The
# Gurobi and Xpress loaders stay on because the core library fails to link
# without the Gurobi one, and they load their library only when asked.
#
# BUILD_DEPS=ON would build bzip2 from its moving master branch, so each
# dependency is chosen by name: the ones upstream pins by tag are built, and
# bzip2 comes from the system.
readonly CMAKE_OPTIONS=(
  -DCMAKE_BUILD_TYPE=Release
  -DBUILD_SHARED_LIBS=ON
  -DBUILD_DEPS=OFF
  -DBUILD_ZLIB=ON
  -DBUILD_absl=ON
  -DBUILD_Protobuf=ON
  -DBUILD_re2=ON
  -DBUILD_Eigen3=ON
  -DBUILD_BZip2=OFF
  -DCMAKE_INSTALL_LIBDIR=lib
  -DBUILD_PYTHON=OFF
  -DBUILD_JAVA=OFF
  -DBUILD_DOTNET=OFF
  -DBUILD_DOC=OFF
  -DBUILD_TESTING=OFF
  -DBUILD_SAMPLES=OFF
  -DBUILD_EXAMPLES=OFF
  -DBUILD_FLATZINC=OFF
  -DBUILD_PACKING=OFF
  -DBUILD_ROUTING=OFF
  -DBUILD_SCHEDULING=OFF
  -DBUILD_MATH_OPT=OFF
  -DUSE_COINOR=OFF
  -DUSE_SCIP=OFF
  -DUSE_HIGHS=OFF
  -DUSE_GLPK=OFF
  -DUSE_PDLP=OFF
  -DUSE_CPLEX=OFF
  -DUSE_GUROBI=ON
  -DUSE_XPRESS=ON
)

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lock_file="${UPSTREAM_LOCK:-$repo_root/upstream.lock}"
source_dir="${UPSTREAM_DIR:-$repo_root/upstream}"

log() {
  printf 'build-reference: %s\n' "$*" >&2
}

fail() {
  log "error: $*"
  exit 1
}

commit="$(sed -n 's/^commit=//p' "$lock_file")"
[[ -n "$commit" ]] || fail "$lock_file has no commit"

prefix="${REFERENCE_PREFIX:-$CACHE_ROOT/reference/$commit}"
build_dir="${REFERENCE_BUILD_DIR:-$CACHE_ROOT/build/$commit}"
jobs="${REFERENCE_JOBS:-$DEFAULT_JOBS}"
stamp="$prefix/$STAMP_NAME"

if [[ -f "$stamp" && "$(cat "$stamp")" == "$commit" ]]; then
  log "$prefix already holds $commit, nothing to build"
  exit 0
fi

grep -qx "commit=$commit" "$source_dir/.commit" 2>/dev/null ||
  fail "$source_dir does not hold $commit, run scripts/fetch-upstream.sh first"

log "configuring in $build_dir"
cmake -S "$source_dir" -B "$build_dir" "${CMAKE_OPTIONS[@]}" \
  -DCMAKE_INSTALL_PREFIX="$prefix"

log "building with $jobs jobs"
cmake --build "$build_dir" --config Release --parallel "$jobs"

log "installing into $prefix"
cmake --install "$build_dir" --config Release

[[ -f "$prefix/include/ortools/sat/cp_model_solver.h" ]] ||
  fail "the install has no include/ortools/sat/cp_model_solver.h"
compgen -G "$prefix/lib/libortools.*" >/dev/null ||
  fail "the install has no ortools library in $prefix/lib"

printf '%s\n' "$commit" >"$stamp"
log "$prefix now holds the reference solver for $commit"
