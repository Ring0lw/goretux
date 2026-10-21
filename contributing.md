# Contributing

Bug fixes, compatibility fixes and performance improvements are always welcome.
Feature additions should be talked through first, open an issue.

## Project overview

goretux is one binary in [`src`](src), grouped into four modules:

- [`os`](src/os) is the low-level system side: [`mem.rs`](src/os/mem.rs) (`/proc/<pid>/mem` access and finding the game), [`uinput.rs`](src/os/uinput.rs) (the virtual mouse) and [`grab.rs`](src/os/grab.rs) (reading your real mouse while the menu is open)
- [`game`](src/game) is everything about the running game
  - [`mod.rs`](src/game/mod.rs) has the reader threads (players, bones, local player) and the writer loop the aim features run in
  - [`il2.rs`](src/game/il2.rs) walks IL2CPP's own metadata (classes, fields and methods by name), [`sdk.rs`](src/game/sdk.rs) resolves everything goretux needs when it attaches
  - [`px.rs`](src/game/px.rs) and [`vis.rs`](src/game/vis.rs) pull the map's static collision out of PhysX and build a BVH for visibility checks
  - [`remote.rs`](src/game/remote.rs) runs Unity's own API on the game's main thread through a small hook (chams and third person use it), [`undo.rs`](src/game/undo.rs) puts every changed value back if goretux crashes or is killed
- [`features`](src/features) is one file per feature: [`aim`](src/features/aim.rs), [`silent`](src/features/silent.rs), [`magic`](src/features/magic.rs), [`rage`](src/features/rage.rs), [`aa`](src/features/aa.rs), [`melee`](src/features/melee.rs), [`chams`](src/features/chams.rs), [`ambiance`](src/features/ambiance.rs), [`mods`](src/features/mods.rs), [`third`](src/features/third.rs), [`hits`](src/features/hits.rs), [`tracers`](src/features/tracers.rs)
- [`ui`](src/ui) is the desktop side: [`overlay.rs`](src/ui/overlay.rs) (the overlay and the menu, Dear ImGui) and [`x11.rs`](src/ui/x11.rs) (tracks the game window, reads hotkeys, makes the overlay click-through)

[`main.rs`](src/main.rs), [`settings.rs`](src/settings.rs) and [`sound.rs`](src/sound.rs) sit at the top.

## Before opening a PR

- `cargo fmt` (settings in [`rustfmt.toml`](rustfmt.toml)), CI checks it
- `cargo build --release` and `cargo test`
- try it in the game, and say which GoreBox build you tested on

## What i won't merge

- **Giant PRs**: PRs that touch lots of different places are hard to review, split them into smaller ones.
- **Injected code**: goretux stays external, a separate process that reads and writes the game's memory and draws its own overlay.
- **Hardcoded addresses**: find things by name or by a byte pattern that's checked before use, like the existing code does, so a game update breaks one feature instead of crashing the game.
