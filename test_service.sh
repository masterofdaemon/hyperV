#!/bin/bash

echo "Test service starting..."
echo "Environment variable TEST_VAR: $TEST_VAR"
echo "Arguments: $@"

# Simple loop to simulate a running service
counter=0
while [ $counter -lt 10 ]; do
    echo "Service running... iteration $counter"
    sleep 2
    counter=$((counter + 1))
done

echo "Test service completed"
