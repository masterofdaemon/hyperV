#!/bin/bash

# Test script for log functionality
echo "This goes to stdout"
echo "Error message" >&2
echo "Another stdout message"
echo "Another error" >&2

# Sleep to keep the process running for a bit
sleep 2

echo "Final stdout message"
echo "Final error message" >&2
