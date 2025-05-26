#!/bin/bash

echo "Testing enhanced hyperV functionality..."

# Build the application
echo "Building hyperV..."
cargo build --release

# Test the new features
echo ""
echo "1. Testing log following (will create a task that generates logs)"

# Create a simple log-generating script
cat > /tmp/log_generator.sh << 'EOF'
#!/bin/bash
echo "Starting log generator..."
for i in {1..10}; do
    echo "Log entry $i at $(date)"
    sleep 1
done
echo "Log generator finished"
EOF

chmod +x /tmp/log_generator.sh

# Test task creation with enhanced fields
echo ""
echo "2. Creating a test task..."
./target/release/hyperV new \
    --name "log-test" \
    --binary "/tmp/log_generator.sh" \
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
timeout 3s ./target/release/hyperV logs log-test --follow || true

echo ""
echo "7. Stopping the task..."
./target/release/hyperV stop log-test

echo ""
echo "8. Final status check..."
./target/release/hyperV status log-test

echo ""
echo "9. Cleaning up..."
./target/release/hyperV remove log-test
rm -f /tmp/log_generator.sh

echo ""
echo "✅ Enhanced functionality test completed!"
