# Asset viewer to game reconstruction

The native viewer is the first executable in this Bevy workspace. It establishes a real rendering path for recovered assets, including skinned model animation. `OriginalAsset` stores source, decoder version and evidence level on the Bevy scene root. The current evidence level is AssetDerived.

There is no reconstructed game loop yet. Asset names, animation names and CSV references cannot establish the original behavior by themselves. Avoid implementing plausible replacement rules and labelling them 1:1.

For each future gameplay subsystem, retain:

1. The original executable function/address or data-table references and their hashes.
2. The decoded inputs, state, update order and outputs, with unresolved fields marked.
3. A deterministic Rust implementation that can run independently of rendering.
4. A recorded original-game input sequence and matching state/visual observations.
5. Comparisons of timing, movement, animation transitions and events before promoting it to GameplayCompared.

Suggested first vertical slice: one character in a small verified environment, with original input mapping, locomotion state transitions and animation selection. Recover simulation tick rate and coordinates before physics. Collision must come from decoded collision data; display triangles are insufficient evidence. Add minigame rules only after their state machines and constants are recovered.

All 836 distinct model files are now accounted for: 828 geometry exports and eight empty draw lists. Individual frontend geometry/UVs and the previously failing model families are decoded. The next priorities are APT timeline/mask composition, exact Wii shader/material behavior, animation timing/static channels, and Havok/VLT object graphs. Material identity, vertex colours, normals and wrapping have executable evidence; remaining shader effects still need reconstruction. The shared JSON/GLB boundary allows Python decoders to be replaced incrementally without rewriting the Bevy viewer or future game systems. See the current [developer handoff](../../docs/HANDOFF.md).

A reference map of the executable's functions, state machines, input schema, menu bridge and minigame rules — with the evidence level of each claim — lives in [`GameMap/`](../../GameMap/README.md); use it to find the addresses and verified models for each gameplay subsystem before starting one.
