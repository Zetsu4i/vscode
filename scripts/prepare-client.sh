#!/usr/bin/env bash
# Prepares the upstream tree for the web-client build:
#   1. copies bridge sources into the upstream tree (they compile with upstream's tsc)
#   2. applies our minimal patches (each documented in patches/README.md)
# The upstream tree stays pristine otherwise. Patches are the ONLY modifications.
set -euo pipefail
cd "$(dirname "$0")/.."

SHA="$(cat UPSTREAM_SHA)"
if [ ! -d upstream/.git ]; then
  echo "upstream missing - run scripts/fetch-upstream.sh first" >&2
  exit 1
fi
CURRENT="$(git -C upstream rev-parse HEAD)"
if [ "$CURRENT" != "$SHA" ]; then
  echo "ERROR: upstream at $CURRENT, pinned $SHA — run scripts/fetch-upstream.sh" >&2
  exit 1
fi

# 1. reset any previous preparation
git -C upstream checkout -- . 2>/dev/null || true
git -C upstream clean -fd src/vs/workbench/contrib/tauriBridge 2>/dev/null || true

# 2. copy bridge sources into the upstream tree
mkdir -p upstream/src/vs/workbench/contrib/tauriBridge
cp -r bridge/src/vs/workbench/contrib/tauriBridge/. upstream/src/vs/workbench/contrib/tauriBridge/

# 3. apply patches in order (absolute path: git -C changes the working dir)
for p in patches/*.patch; do
  [ -e "$p" ] || continue
  echo "applying $(basename "$p")"
  git -C upstream apply --whitespace=nowarn "$PWD/$p"
done

# 4. vendor the product.json the backbone will serve to the client
cp upstream/product.json resources/product.json

echo "client tree prepared"
