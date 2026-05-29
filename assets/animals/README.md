# Animal sprites

Drop PNGs here named `<species_id>.png` (e.g. `mouse.png`, `fox.png`,
`polar_bear.png`, `frox.png`). They get scanned by `build.rs` and embedded
into the binary at compile time via `include_bytes!`.

Recommended size: 96×96 px or 128×128 px with a transparent background.

Missing species fall back to a placeholder showing the first letter of the
species' display name, so you can add art incrementally. Special filename
`_mystery.png` renders in the
crossbreeding "outcome" slot when the predicted offspring should be hidden;
absent, the UI falls back to a "?" character.

Species ids match the catalog in [`src/game/species.rs`](../../src/game/species.rs):

- mouse, frog, penguin, monkey, lion, fox, otter, polar_bear, toucan, zebra
- Hybrids: frox, otterfly, snow_lion, monkan, zebra_lion
