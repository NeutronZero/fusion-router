#!/usr/bin/env bash
set -euo pipefail

# setup-opencode.sh — Configure OpenCode to use FusionRouter as its provider

FUSION_URL="${FUSION_URL:-http://localhost:8080}"
API_KEY="${FUSION_ROUTER_API_KEY:-}"

echo "Checking FusionRouter at $FUSION_URL..."
if curl -sf "$FUSION_URL/health" > /dev/null 2>&1; then
    echo "  ✓ FusionRouter is running"
else
    echo "  ✗ FusionRouter not reachable at $FUSION_URL"
    echo "    Start it with: cargo run"
    echo "    Then re-run this script."
    exit 1
fi

OPENCODE_DIR="${XDG_CONFIG_HOME:-$HOME/.config}/opencode"
mkdir -p "$OPENCODE_DIR"

CONFIG_FILE="$OPENCODE_DIR/project.json"

if [ -f "$CONFIG_FILE" ]; then
    echo "  Found existing $CONFIG_FILE — merging provider section..."
    # Use jq if available, otherwise overwrite
    if command -v jq &> /dev/null; then
        tmp=$(mktemp)
        jq ".provider = {\"baseURL\": \"$FUSION_URL/v1\", \"apiKey\": \"$API_KEY\"}" "$CONFIG_FILE" > "$tmp"
        mv "$tmp" "$CONFIG_FILE"
    else
        cat > "$CONFIG_FILE" <<-EOF
{
  "provider": {
    "baseURL": "$FUSION_URL/v1",
    "apiKey": "$API_KEY"
  }
}
EOF
    fi
else
    cat > "$CONFIG_FILE" <<-EOF
{
  "provider": {
    "baseURL": "$FUSION_URL/v1",
    "apiKey": "$API_KEY"
  }
}
EOF
fi

echo ""
echo "  ✓ OpenCode configured to use FusionRouter at $FUSION_URL"
echo ""
echo "Next steps:"
echo "  1. Restart OpenCode to pick up the new config."
echo "  2. Start chatting — FusionRouter handles model selection automatically."
echo "  3. (Optional) Set FUSION_ROUTER_API_KEY env var for authenticated access."
