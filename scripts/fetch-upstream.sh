#!/usr/bin/env bash
# Fetches the OR-Tools source pinned in upstream.lock into upstream/.
#
# The archive must match the SHA-256 in the lock. GitHub can produce other
# bytes for the same commit, so on a mismatch the script fetches the commit
# with git and accepts it when its tree hash matches the lock.
#
# The last step, on every path, checks that the proto files committed under
# crates/proto/proto are byte for byte the ones in the fetched source.
#
# Environment:
#   UPSTREAM_LOCK  lock file to read (default: upstream.lock in the repo root)
#   UPSTREAM_DIR   target directory (default: upstream/ in the repo root)
set -euo pipefail

readonly DOWNLOAD_RETRIES=5
readonly DOWNLOAD_RETRY_DELAY_SECONDS=5
readonly DOWNLOAD_MAX_SECONDS=600
readonly STAMP_NAME=.commit
readonly COMMITTED_PROTO_DIR=crates/proto/proto
readonly COMMITTED_PROTOS=(
  ortools/sat/cp_model.proto
  ortools/sat/sat_parameters.proto
)

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
lock_file="${UPSTREAM_LOCK:-$repo_root/upstream.lock}"
target_dir="${UPSTREAM_DIR:-$repo_root/upstream}"
work_dir=""

log() {
  printf 'fetch-upstream: %s\n' "$*" >&2
}

fail() {
  log "error: $*"
  exit 1
}

cleanup() {
  if [[ -n "$work_dir" && -d "$work_dir" ]]; then
    rm -rf "$work_dir"
  fi
}
trap cleanup EXIT

lock_value() {
  local key="$1" value
  value="$(sed -n "s/^${key}=//p" "$lock_file")"
  [[ -n "$value" ]] || fail "$lock_file has no value for '$key'"
  printf '%s' "$value"
}

sha256_of() {
  if command -v sha256sum >/dev/null 2>&1; then
    sha256sum "$1" | cut -d ' ' -f 1
  else
    shasum -a 256 "$1" | cut -d ' ' -f 1
  fi
}

# Prints the stamp a checked source gets: the commit and the hash that passed.
stamp_text() {
  printf 'commit=%s\nverified=%s\n' "$1" "$2"
}

# The stamp matches when the commit and the hash that passed are still the
# ones in the lock.
stamp_matches() {
  local stamp="$target_dir/$STAMP_NAME"
  [[ -f "$stamp" ]] || return 1
  local current
  current="$(cat "$stamp")"
  [[ "$current" == "$(stamp_text "$commit" "sha256:$archive_sha256")" ]] && return 0
  [[ "$current" == "$(stamp_text "$commit" "tree:$tree")" ]] && return 0
  return 1
}

# Unpacks the archive into $1 when its SHA-256 matches the lock.
fetch_archive() {
  local dest="$1" archive="$work_dir/archive.tar.gz" actual
  log "downloading $repo/archive/$commit.tar.gz"
  if ! curl --fail --silent --show-error --location --proto '=https' --tlsv1.2 \
    --retry "$DOWNLOAD_RETRIES" --retry-delay "$DOWNLOAD_RETRY_DELAY_SECONDS" \
    --max-time "$DOWNLOAD_MAX_SECONDS" \
    --output "$archive" "$repo/archive/$commit.tar.gz"; then
    log "download failed"
    return 1
  fi
  actual="$(sha256_of "$archive")"
  if [[ "$actual" != "$archive_sha256" ]]; then
    log "archive SHA-256 is $actual, the lock says $archive_sha256"
    return 1
  fi
  log "archive SHA-256 matches the lock"
  mkdir -p "$dest"
  tar -xzf "$archive" -C "$dest" --strip-components 1
}

# Checks the commit out into $1 when its tree hash matches the lock.
fetch_git() {
  local dest="$1" clone="$work_dir/clone" actual
  log "fetching $commit with git"
  git init --quiet "$clone"
  if ! git -C "$clone" fetch --quiet --depth 1 "$repo" "$commit"; then
    log "git fetch failed"
    return 1
  fi
  actual="$(git -C "$clone" rev-parse 'FETCH_HEAD^{tree}')"
  if [[ "$actual" != "$tree" ]]; then
    log "tree hash is $actual, the lock says $tree"
    return 1
  fi
  log "tree hash matches the lock"
  mkdir -p "$dest"
  git -C "$clone" archive --format=tar FETCH_HEAD | tar -xf - -C "$dest"
}

# Fails when a committed proto file differs from the same file in the source.
check_committed_protos() {
  local proto differs=0
  for proto in "${COMMITTED_PROTOS[@]}"; do
    if ! cmp -s "$repo_root/$COMMITTED_PROTO_DIR/$proto" "$target_dir/$proto"; then
      log "$COMMITTED_PROTO_DIR/$proto differs from $target_dir/$proto"
      differs=1
    fi
  done
  [[ "$differs" -eq 0 ]] || fail "committed proto files differ from the fetched source"
  log "committed proto files match the fetched source"
}

[[ -f "$lock_file" ]] || fail "lock file $lock_file does not exist"
repo="$(lock_value repo)"
commit="$(lock_value commit)"
archive_sha256="$(lock_value archive_sha256)"
tree="$(lock_value tree)"
[[ "$repo" == https://* ]] || fail "repo must be an https URL, got '$repo'"

if stamp_matches; then
  log "$target_dir already holds $commit, nothing to do"
  check_committed_protos
  exit 0
fi

# The work directory sits beside the target, so the final move is a rename.
mkdir -p "$(dirname "$target_dir")"
work_dir="$(mktemp -d "$(dirname "$target_dir")/.fetch-upstream.XXXXXX")"
staged="$work_dir/staged"

if fetch_archive "$staged"; then
  verified="sha256:$archive_sha256"
else
  rm -rf "$staged"
  if fetch_git "$staged"; then
    verified="tree:$tree"
  else
    fail "neither the archive SHA-256 nor the git tree hash matches $lock_file"
  fi
fi

stamp_text "$commit" "$verified" >"$staged/$STAMP_NAME"
rm -rf "$target_dir"
mv "$staged" "$target_dir"
log "$target_dir now holds $commit ($verified)"
check_committed_protos
