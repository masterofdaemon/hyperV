#!/bin/bash
counter=1
while [ $counter -le 20 ]; do
    echo "Output line $counter at $(date)"
    sleep 1
    counter=$((counter + 1))
done
