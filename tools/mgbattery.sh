#!/bin/bash
# Run every minigame in the PowerPC VM lab from launch to the post-game screen and back to the world, in parallel.
# usage: tools/mgbattery.sh [frames]   (build first: cargo build --profile lab --bin mglab)
# Computer players play where the game allows it (EAGL_MG_ALLAI); RcCars and Paper Airplanes need a human, so scripted
# input stands in (hold accelerate / one throw).  Summary: frame of the PostGame screen and of the exit, or the error.
cd "$(dirname "$0")/../_bevy" || exit 1
F=${1:-12000}
OUT=../scratch_battery
mkdir -p $OUT
run() { # type, extra env...
  local t=$1; shift
  env EAGL_MG_POSTGAME=done EAGL_MG_FRAMES=$F "$@" timeout 1800 ./target/lab/mglab.exe probe $t > $OUT/$t.txt 2>&1
}
for t in 0 2 3 4 6 8; do run $t EAGL_MG_ALLAI=1 & done
run 1 EAGL_MG_PADS="100-99999:0800" &
run 5 EAGL_MG_ACC="700-703:512,962,616" &
wait
names=(Dart RcCars Tetherball Dodgeball Footie Paper Wallball x FreeThrow)
for t in 0 1 2 3 4 5 6 8; do
  pg=$(grep -m1 'OpenAptScreen \[Str("PostGame")\]' $OUT/$t.txt | grep -oE '\[[0-9]+\]')
  ex=$(grep -m1 'minigame object .* -> 0x0' $OUT/$t.txt | grep -oE '^ *\[[0-9]+\]' | tr -d ' ')
  err=$(grep -m1 -E '^frame [0-9]+:|launch failed|boot failed' $OUT/$t.txt | cut -c1-150)
  printf "%-11s postgame %-8s exit %-8s %s\n" "${names[$t]}" "${pg:--}" "${ex:--}" "$err"
done
