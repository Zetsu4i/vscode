#!/usr/bin/env bash
# Builds the real VSCode web client (gulp vscode-web) from the prepared upstream tree.
# Output: ../vscode-web (relative to upstream/) — served by the Rust backbone.
set -euo pipefail
cd "$(dirname "$0")/../upstream"

echo "installing dependencies (npm ci)..."
npm ci --no-audit --no-fund 2>&1 | tail -2

echo "building vscode web client (gulp vscode-web-ci)..."
# the -ci task is what upstream's own product build uses (esbuild path);
# the non-ci task runs the private "angler" mangler which trips over
# upstream's own protected-field accesses (vs/sessions)
npm run gulp vscode-web-ci 2>&1 | tail -15

# the backbone serves product.json to the workbench (productConfiguration)
cp product.json ../vscode-web/product.json

echo "client build complete: $(pwd)/../vscode-web"
