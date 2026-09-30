# Playground jump command is inert in this executable

The native controls table emits `EVENT_PLAYER_JUMP` (action 2), but that does not
produce a jump impulse in this `playgroundz.elf` (SHA-256
`5cef3efc7005fb71fed0a75e60ee240ee6ac4243b00dd3296e6e53ca269a3e2c`).
This conclusion concerns the playground player path in this image; Footie has
its own jump state machine and is outside it.

- `PlaygroundWorld::HandleActions` checks action 2 at `0x803de304–0x803de314`.
  When active, `0x803de328` calls `LocalCharacterControl::Jump` with frame delta
  and two zero floats.
- `LocalCharacterControl::Jump` (`0x802eeac8`, 96 bytes) checks its refcount and
  enqueues a 20-byte command with kind **4**, delta and the two float values.
- `LocalCharacterControl::Update` (`0x802eeb28`, 2604 bytes) dispatches kinds
  0 (analog), 1 (digital), 2 (stop), and 3 (face direction). Kind 4 follows
  `0x802eebf4 → 0x802eebf8 → 0x802eebfc → 0x802eec00 → 0x802eec1c →
  0x802eec20 → 0x802eec24 → 0x802eec28 → 0x802ef09c`, bypassing every command
  handler. Queue count clears at `0x802ef4c8`.
- The only direct Jump call found in the original game code is the playground
  HandleActions call. The LocalCharacterControl class has no other queue consumer.
- `PhysicsDynamicCharacter::BuildCharacterInput` independently sets the Havok
  wants-jump input byte to zero at `0x803b687c–0x803b688c`. The Havok Jumping state
  is registered by the dynamic character constructor at `0x803b60e0`, but this
  input path does not request it. Its library default jump height (1.5 at
  `0x801dc8b0`) is therefore not a recovered playground jump impulse.

`tools/jump_command_oracle.py --check` executes the **full original** Jump and
LocalCharacterControl Update functions for deltas 0, 1, 16, 60 and 200 ms. Each
jump case enqueues one kind-4 command, follows the exact bypass trace above, and
consumes the queue. Complete 256-byte movement and character-state snapshots,
plus smoothing/idle/facing words, are identical to a no-command baseline. A
movement-speed sentinel (3.25, bits `0x40500000`) remains unchanged.

There are no hooks for the command producer, consumer, or arithmetic. The oracle
adds interpreter support for stack-update store and floating comparison, and
ignores only compiler paired-single register restores (the following scalar
loads restore the FPRs). Golden fixtures contain data snapshots and instruction
addresses, not executable bytes. The fixture pins the executable hash.

A faithful playground adapter may continue exposing action 2 diagnostically,
while producing no airborne response. Adding a vertical impulse would be a new
behavior. Collision/gravity integration remains a separate reconstruction task.
