#!/usr/bin/env bash
set -euo pipefail

if [[ "${GITHUB_REF:-}" != refs/heads/main ]]; then
  echo 'Releases must be dispatched from main.' >&2
  exit 1
fi

version=$(node --input-type=module <<'JS'
import { readFileSync } from 'node:fs';
const json = (path) => JSON.parse(readFileSync(path, 'utf8'));
const version = json('package.json').version;
if (typeof version !== 'string' || !/^(0|[1-9]\d*)\.(0|[1-9]\d*)\.(0|[1-9]\d*)$/.test(version)) {
  throw new Error('Release version must be a stable major.minor.patch version.');
}
const cargo = readFileSync('src-tauri/Cargo.toml', 'utf8').match(/^version = "([^"]+)"$/m)?.[1];
if (json('src-tauri/tauri.conf.json').version !== version || cargo !== version) {
  throw new Error('Release versions disagree.');
}
console.log(version);
JS
)
tag="v$version"

# Listing includes drafts; API/auth failures must not be treated as absence.
releases=$(gh api --paginate --slurp "repos/$GITHUB_REPOSITORY/releases?per_page=100")
release_exists=$(jq -r --arg tag "$tag" 'any(.[][]; .tag_name == $tag)' <<< "$releases")
if [[ "$release_exists" == true ]]; then
  echo "Release $tag already exists (possibly a draft)." >&2
  exit 1
fi
refs=$(gh api "repos/$GITHUB_REPOSITORY/git/matching-refs/tags/$tag")
tag_exists=$(jq -r --arg ref "refs/tags/$tag" 'any(.[]; .ref == $ref)' <<< "$refs")
if [[ "$tag_exists" == true ]]; then
  echo "Tag $tag already exists; inspect it before retrying." >&2
  exit 1
fi

printf 'version=%s\ntag=%s\n' "$version" "$tag" >> "$GITHUB_OUTPUT"
