#!/bin/bash

# Example usage of hyperV service manager

echo "Building hyperV..."
cargo build --release

HYPERV="./target/release/hyperV"

echo -e "\n=== hyperV Service Manager Examples ===\n"

# Clean up any existing tasks
echo "Cleaning up existing tasks..."
$HYPERV remove test-service 2>/dev/null || true
$HYPERV remove log-service 2>/dev/null || true
$HYPERV remove web-server 2>/dev/null || true

echo -e "\n1. Creating a simple test service..."
$HYPERV new --name "test-service" \
    --binary "/Users/masutanoakira/projs/hyperV/test_service.sh" \
    --args "hello" --args "world" \
    --env "TEST_VAR=example_value" \
    --auto-restart

echo -e "\n2. Creating a log service that writes to a file..."
cat > /tmp/log_service.sh << 'EOF'
#!/bin/bash
echo "Log service starting at $(date)"
for i in {1..20}; do
    echo "$(date): Log entry #$i" >> /tmp/hyperv_test.log
    sleep 1
done
echo "Log service completed at $(date)"
EOF
chmod +x /tmp/log_service.sh

$HYPERV new --name "log-service" \
    --binary "/tmp/log_service.sh" \
    --env "LOG_LEVEL=INFO"

echo -e "\n3. Creating a simple Python HTTP server..."
cat > /tmp/simple_server.py << 'EOF'
#!/usr/bin/env python3
import http.server
import socketserver
import os

PORT = int(os.environ.get('PORT', 8000))

class MyHTTPRequestHandler(http.server.SimpleHTTPRequestHandler):
    def do_GET(self):
        if self.path == '/':
            self.send_response(200)
            self.send_header('Content-type', 'text/html')
            self.end_headers()
            self.wfile.write(b'''
            <html><body>
            <h1>hyperV Test Server</h1>
            <p>This server is managed by hyperV!</p>
            <p>Environment: ''' + os.environ.get('APP_ENV', 'development').encode() + b'''</p>
            </body></html>
            ''')
        else:
            super().do_GET()

with socketserver.TCPServer(("", PORT), MyHTTPRequestHandler) as httpd:
    print(f"Server running on port {PORT}")
    httpd.serve_forever()
EOF
chmod +x /tmp/simple_server.py

$HYPERV new --name "web-server" \
    --binary "/usr/bin/python3" \
    --args "/tmp/simple_server.py" \
    --env "PORT=8080" \
    --env "APP_ENV=production" \
    --auto-restart

echo -e "\n=== Current Tasks ===\n"
$HYPERV list

echo -e "\n=== Starting Services ===\n"

echo "Starting test-service..."
$HYPERV start test-service

echo "Starting log-service..."
$HYPERV start log-service

echo "Starting web-server..."
$HYPERV start web-server

echo -e "\n=== Services Status ===\n"
$HYPERV status

echo -e "\n=== Waiting 5 seconds... ===\n"
sleep 5

echo "Current status:"
$HYPERV list

echo -e "\n=== Checking log file ===\n"
if [ -f /tmp/hyperv_test.log ]; then
    echo "Log file contents (last 5 lines):"
    tail -5 /tmp/hyperv_test.log
fi

echo -e "\n=== Testing web server ===\n"
echo "Web server should be running on http://localhost:8080"
echo "You can test it with: curl http://localhost:8080"

echo -e "\n=== Stopping all services ===\n"
$HYPERV stop test-service
$HYPERV stop log-service
$HYPERV stop web-server

echo -e "\n=== Final Status ===\n"
$HYPERV list

echo -e "\n=== Example completed! ===\n"
echo "You can now manage your own services with hyperV."
echo "Try: $HYPERV new --help"
