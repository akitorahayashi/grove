#!/usr/bin/env bash

set -euo pipefail

release_tag="${1:?usage: verify-release-version.sh <v-tag> [repository-root]}"
repository_root="${2:-.}"
metadata="$(cargo metadata \
  --manifest-path "$repository_root/Cargo.toml" \
  --format-version 1 \
  --no-deps)"
workspace_package_count="$(jq -r '.workspace_members | length' <<<"$metadata")"

if [[ "$workspace_package_count" != 1 ]]; then
  echo "release verification requires exactly one workspace package; found $workspace_package_count" >&2
  exit 1
fi

workspace_package_id="$(jq -r '.workspace_members[0]' <<<"$metadata")"
package_version="$(jq -r --arg id "$workspace_package_id" \
  '.packages[] | select(.id == $id) | .version' <<<"$metadata")"
expected_tag="v${package_version}"

if [[ "$release_tag" != "$expected_tag" ]]; then
  echo "release tag '$release_tag' does not match package version '$package_version' (expected '$expected_tag')" >&2
  exit 1
fi

echo "release tag '$release_tag' matches package version '$package_version'"
