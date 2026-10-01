#!/bin/bash
# usage: run_flow.sh "<apt-script>" <shot-secs> <out.png> [extra args]
cd /d/_eagl/_bevy
rm -f docs/apt-log.txt
S="$1"; T="$2"; O="$3"; shift 3
timeout $((T+60)) ./target/release/EAGL-Workbench.exe --mode apt --apt main --apt-fwprint --apt-script "$S" --shot "$O" --shot-at $T "$@" > /tmp/o4.txt 2>&1
