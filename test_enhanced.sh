#!/bin/bash
set -euo pipefail

echo "Testing enhanced hyperV functionality..."

# Build the application
echo "Building hyperV..."
cargo build --release

# Test the new features
echo ""
echo "1. Testing log following (will create a task that generates logs)"

# Create a simple log-generating script
LOG_GENERATOR=$(mktemp /tmp/log_generator.XXXXXX)
cat > "$LOG_GENERATOR" << 'EOF'
#!/bin/bash
echo "Starting log generator..."
for i in {1..10}; do
    echo "Log entry $i at $(date)"
    sleep 1
done
echo "Log generator finished"
EOF

chmod +x "$LOG_GENERATOR"

# Test task creation with enhanced fields
echo ""
echo "2. Creating a test task..."
./target/release/hyperV new \
    --name "log-test" \
    --binary "$LOG_GENERATOR" \
    --auto-restart

echo ""
echo "3. Starting the task..."
./target/release/hyperV start log-test

echo ""
echo "4. Checking status (should show new fields)..."
./target/release/hyperV status log-test

echo ""
echo "5. Showing logs..."
./target/release/hyperV logs log-test --lines 5

echo ""
echo "6. Testing log following for 3 seconds..."
# Portable replacement for `timeout 3s` (not shipped on macOS by default).
./target/release/hyperV logs log-test --follow &
FOLLOW_PID=$!
sleep 3
kill "$FOLLOW_PID" 2>/dev/null || true
wait "$FOLLOW_PID" 2>/dev/null || true

echo ""
echo "7. Stopping the task..."
./target/release/hyperV stop log-test

echo ""
echo "8. Final status check..."
./target/release/hyperV status log-test

echo ""
echo "9. Cleaning up..."
./target/release/hyperV remove log-test
rm -f "$LOG_GENERATOR"

echo ""
echo "✅ Enhanced functionality test completed!"
