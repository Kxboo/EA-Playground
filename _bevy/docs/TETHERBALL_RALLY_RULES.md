# Tetherball rally rules

These helpers are reconstructed from `Remaster/reference/playgroundz.elf`, SHA-256
`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.
The independent instruction emulator executes each original function; Rust does
not generate the fixture expectations. `tools/tetherball_rally_rules_oracle.py
--check` compares 511 complete native calls against the committed fixture.

| Original entry | Recovered behavior |
| --- | --- |
| GetAIHitAttempt `0x8039b894` | Dispatches GAME+234 through the executable's table at `0x804dd0a4`; preserves output references unless explicitly stored. |
| GetAIPowerHitType `0x8039b968` | GAME+238 values 1/3/7 map to power types 2/1/3; all others return 0. Player argument is unused. |
| CalculateTetherballZone `0x80399c1c` | Table at `0x804420bc`, or RNG for hit type 7; always stores hit type to +224, replacing it with 4 when the returned zone is 2. |
| IncrementChargeMeter `0x8039c414` | Unsigned charge comparison, wrapping addition of +30c, ordered HUD/audio effects, then unsigned cap at 5. |
| ConsumeCharge `0x8039c4c0` | Unsigned available/required comparison; succeeds for zero consumption and emits its HUD call. |
| CheckForBallDrop `0x8039b9ac` | Wrapping increment +270; exactly value 3 executes DropOneZone then AdjustCameraHeight. |

GetAIHitAttempt uses the existing `tetherball_gestures::HitAttempt`. Selector -1
only replaces power type using GetAIPowerHitType. Selectors 0/1 set strike and
power 0/2; 2/3 set reverse strike and power 0/1; 7 sets both strikes and power 3.
Those five strike selectors set the +229 marker. Selectors 4/5/6 and all other
values leave every output untouched. It never clears a previously true output.

`RallyRuleState` owns only previously missing GAME+234/+238 and AI entity+70
shadows. Charge storage remains `Lifecycle.mega_values` (+314) and
`mega_states` (+30c). Casts preserve their raw 32-bit unsigned meaning.
Both charge functions synchronize AI+70 even on failed consumption or unchanged
charge. Increment's first HUD call reports the wrapping sum before capping;
the frontend sound 34 occurs only when addition reaches unsigned >=5. A charge
already above 5 is capped and reported without that sound. Negative signed
increments are therefore not treated as a conventional saturating meter.

CalculateTetherballZone supports the native meaningful table domain: ball zones
0/1 and hit types 0..3, plus RNG hit type 7. Its two rows are `[0,1,2,2]` and
`[2,2,1,0]`. Type 7 requests the external inclusive random range 0..1; zero
returns zone 1 and nonzero returns zone 0. Other raw table indices read adjacent
executable memory, which this API deliberately does not reinterpret as game
rules. Callers must retain this domain boundary.

Ball dropping reuses the already recovered `BallMotion::drop_one_zone`: only
zone 1 changes to 0, but both angular velocities always receive the original
five-percent damping, and target height is updated. It then reuses the complete
`Lifecycle::adjust_camera_height` at `0x8039b504`, including current-distance
selection, native offset constants and ordered target/position calls over 600ms.
The support domain is current distance 0..2. Ball/AI/player indices remain the
valid original two-player domain; arbitrary native pointer accesses are outside
this safe projection.

The fixture covers retained outparams, all dispatch arms, signed selector
extremes, both players, zero/insufficient/exact consumption, unsigned extremes,
negative and overflowing charge increments, every table cell, both RNG results,
and drop-counter equality/wrap with every zone/distance. Tests compare full
Lifecycle serde, all BallMotion bits, full rule state, outputs, hit type/drop
counter, meaningful function returns and ordered engine effects. Void return
registers are intentionally not asserted as semantic outputs.

The oracle hooks only external RNG, HUD, audio and camera services inherited
from LifecycleEmu. GetAIHitAttempt's nested power helper, zone table loads,
charge routines, DropOneZone, SetZone/target-height behavior and camera offset
arithmetic execute original instructions. This is instruction-emulation
evidence for this executable; it is not a claim of console hardware validation
or a complete AI policy, collision proxy, Return or Accelerate state handler.
