#!/bin/bash
set -e
cd "$(dirname "$0")"

# Build if needed
if [ ! -f target/debug/ironcrm ]; then
    echo "Building..."
    cargo build
fi

# Kill any existing instance
pkill -f "target/debug/ironcrm" 2>/dev/null || true

# Start server
echo "Starting IronCRM on http://127.0.0.1:3001"
PORT=3001 ./target/debug/ironcrm &
SERVER_PID=$!

# Wait for it
sleep 2

# Open browser
open "http://127.0.0.1:3001"

# Wait for server
echo "Server PID: $SERVER_PID"
echo "Press Ctrl+C to stop"
wait $SERVER_PID
