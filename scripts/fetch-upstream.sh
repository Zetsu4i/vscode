#!/usr/bin/env bash
# Fetches the pristine microsoft/vscode source at the pinned commit into ./upstream/
# The upstream tree is NEVER committed to this repository. It is the copy source.
set -euo pipefail
cd "$(dirname "$0")/.."

SHA="$(cat UPSTREAM_SHA)"
TAG="$(cat UPSTREAM_TAG)"
DIR="upstream"

if [ -d "$DIR/.git" ]; then
  CURRENT="$(git -C "$DIR" rev-parse HEAD)"
  if [ "$CURRENT" = "$SHA" ]; then
    echo "upstream already at pinned SHA $SHA"
    exit 0
  fi
  echo "upstream at $CURRENT != pinned $SHA — refetching"
  rm -rf "$DIR"
fi

echo "cloning microsoft/vscode at tag $TAG (sha $SHA)..."
git clone --depth 1 --branch "$TAG" --single-branch https://github.com/microsoft/vscode "$DIR"
ACTUAL="$(git -C "$DIR" rev-parse HEAD)"
if [ "$ACTUAL" != "$SHA" ]; then
  echo "ERROR: cloned SHA $ACTUAL does not match pinned $SHA" >&2
  exit 1
fi
echo "upstream ready at $SHA"
