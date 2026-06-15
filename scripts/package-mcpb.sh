#!/usr/bin/env bash
set -euo pipefail

ROOT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VERSION="$(sed -n 's/^version = "\(.*\)"/\1/p' "$ROOT_DIR/Cargo.toml" | head -1)"
BUNDLE_DIR="$ROOT_DIR/dist/mcpb/ironcrm"
OUTPUT="$ROOT_DIR/dist/ironcrm-mcp-$VERSION-darwin.mcpb"

cd "$ROOT_DIR"
cargo build --release

rm -rf "$BUNDLE_DIR"
mkdir -p "$BUNDLE_DIR/server"

cp "$ROOT_DIR/target/release/ironcrm" "$BUNDLE_DIR/server/ironcrm"
chmod 755 "$BUNDLE_DIR/server/ironcrm"
cp "$ROOT_DIR/packaging/mcpb/manifest.json" "$BUNDLE_DIR/manifest.json"
cp "$ROOT_DIR/MCP.md" "$BUNDLE_DIR/README.md"

npx -y @anthropic-ai/mcpb validate "$BUNDLE_DIR/manifest.json"
rm -f "$OUTPUT"
npx -y @anthropic-ai/mcpb pack "$BUNDLE_DIR" "$OUTPUT"
npx -y @anthropic-ai/mcpb info "$OUTPUT"

echo "$OUTPUT"
