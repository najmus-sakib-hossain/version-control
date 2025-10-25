#!/bin/bash
DX_WATCH_PROFILE=1 cargo run &
PID=$!
sleep 2
# First change
touch README.md
sleep 1
# Second change (should be cached)
echo " " >> README.md  
sleep 1
# Third change
echo " " >> README.md
sleep 1
kill $PID 2>/dev/null
