# Tetherball return and accelerate graph

`tetherball_rally.rs` reconstructs `UpdateReturn` (`0x80398368`, 1,820 bytes)
and `UpdateAccelerate` (`0x80398a84`, 1,668 bytes) from the pinned retail ELF.
It composes the recovered ball, lifecycle, gesture, animation, charge, hit,
multiplier, indicator and pause routines. Engine animation, controller, audio,
camera, particle and front-end operations remain synchronous service calls.
This is recovered gameplay logic; the Bevy world does not yet expose a playable
tetherball minigame.

The handlers preserve the distinction between a scheduled hit and a scheduled
miss. Frame deltas subtract with 32-bit wrapping, and the resulting timer is
compared as signed. Successful delayed hits increment attempt/hit statistics;
Accelerate also increments the multiplier before executing the hit. Misses
retain the native charge-feedback flag and attached-controller effect gates.

| Decision | Return | Accelerate |
| --- | --- | --- |
| Narrow hit window tables | +360 / +36c | +378 / +384 |
| Broader indicator window | First width becomes pi/2 | Second width becomes pi/2 |
| Ordinary strike delay | 110 ms | 160 ms |
| Mega strike delay | 110 ms | 110 ms |
| Prepared speed | Native base/power/mega calculation | Same, then multiply by +358 |
| Prepared ball direction | Reverse current direction | Preserve current direction |
| Leaving the return interval | Hit returns to state 28; miss drops/checks ball and enters 29 | Enters state 28 |

Human input samples the original repeated controller reads in order. An event
may change between reads; the fixture includes sequenced responses. AI input
uses the recovered +234/+238 dispatch. Charge is consumed before starting the
hit animation, including a failed attempt outside the narrow hit window. A
five-charge mega attempt sets its pending feedback flags before range testing.
The normal power-type-2 path updates base speed only when base plus bonus
exceeds base plus 0.3, then clears the bonus. It is not a generic speed clamp.

Shared fields stay with `Lifecycle`, `ResetState`, `GestureState`, `BallMotion`
and `ServeState`. `HitState`, `HitAnimations` and `RallyRuleState` supply the
additional recovered fields. Character effect positions use an explicit host
vector for the BSS global at `0x805e37d0`; this work does not infer its runtime
initializer value. The final pause-controller loop runs after state transitions.

## Evidence

`tools/tetherball_rally_oracle.py --check` executes **2,086 original calls**.
No gameplay predicate or nested hit/charge/animation/zone decision is replaced
with a supplied result. The oracle records complete lifecycle/ball state,
additional words/bytes, speed tunables, AI charge mirrors and ordered effects.
Both linked calls and tail branches to engine services are intercepted.

The corpus reaches every instruction and both outcomes of every conditional
branch in these five tracked bodies:

| Body | Instructions | Conditional outcomes |
| --- | ---: | ---: |
| UpdateReturn | 455 / 455 | 86 |
| UpdateAccelerate | 417 / 417 | 80 |
| HitTetherball | 387 / 387 | 40 |
| IncrementMultiplier | 125 / 125 | 16 |
| DrawHitIndicatorParticle | 105 / 105 | 12 |

Five Rust tests compare the original snapshots, including floating-point bit
patterns and effect ordering. Separate [rally-rule](TETHERBALL_RALLY_RULES.md)
and [animation](TETHERBALL_HIT_ANIMATION.md) corpora cover 511 and 1,663
public-helper calls respectively (4,260 new original-code comparisons overall).
The original executable SHA-256 is
`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`.

Coverage measures executed branches in these bodies, not every possible input
combination or console hardware equivalence. The safe projection uses players
0/1, distances 0..2 and valid ball zones 0/1. Zone result 2 schedules a miss and
must not enter `HitTetherball`. Pointer aliasing, invalid indices, arbitrary
NaNs/FPSCR modes, and the engine implementations behind service calls are not
claimed as reconstructed. The indicator oracle models nonzero divided by zero
as signed infinity; 0/0 remains outside its supported floating-point domain.
