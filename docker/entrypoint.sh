#!/bin/sh
set -e

/usr/local/bin/orthanc_server &
SERVER_PID=$!

cd /usr/local/app/ui
./server &
UI_PID=$!

term() {
  kill -TERM "$SERVER_PID" "$UI_PID" 2>/dev/null || true
}
trap term TERM INT

wait -n "$SERVER_PID" "$UI_PID"
EXIT_CODE=$?
term
wait || true
exit $EXIT_CODE
