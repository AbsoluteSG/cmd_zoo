# Sound effects (`assets/sfx/`)

Every audio clip the game plays lives here. The build script
([`../../build.rs`](../../build.rs)) scans this folder and embeds each clip into
the executable as a `(id, bytes)` table; at startup
[`src/audio.rs`](../../src/audio.rs) decodes them all and plays them by **id**.

There is no manifest to edit — **drop a file in, rebuild, and it's available.**
A clip that no code references is simply never played; a sound id that no file
provides is a **silent no-op** (so the game runs fine before any audio art is
added, and a missing cue never crashes or logs).

## Supported formats

- `.ogg` (preferred — small, royalty-free)
- `.wav`

Other extensions in this folder are ignored.

## The id is the file name

A clip's **id is its file stem** (the name without the extension):
`blue_frog_poke.ogg` → id `blue_frog_poke`.

Matching is **fuzzy**: case-insensitive, and `_` / `-` are ignored. So
`blue_frog_poke`, `blueFrogPoke`, and `blue-frog-poke` all resolve to the same
clip. This means a file named with a species' snake_case id lines up with code
that builds the id from that same species id.

## Naming conventions

Cues fall into two groups.

### Per-animal cues — `{species_id}_<cue>`

Prefix the clip with the animal's **species id** (its catalog id, e.g. `lion`,
`blue_frog`, `red_fox`), then the cue name:

| Pattern               | When it plays                                              | Example                |
| --------------------- | --------------------------------------------------------- | ---------------------- |
| `{species_id}_poke`   | The player interacts with one of their animals — clicking it (poke / collect) or pressing **E** to inspect it. | `blue_frog_poke.ogg`   |

Add the cue for a new species by dropping `{species_id}_poke.ogg` here — no code
change. Species without a clip just play nothing on poke.

### Global cues — `<name>_sfx`

Not tied to a species; one shared clip:

| Id              | When it plays                                  |
| --------------- | ---------------------------------------------- |
| `income_sfx`    | Collecting an animal's accrued income (at cap). |
| `poke_lion_sfx` | Placeholder impact cue when a wild animal hits the player (Bash / Venom / Throw). |

## Adding a sound

1. Export to `.ogg` (or `.wav`).
2. Name it by the convention above (e.g. `lion_poke.ogg`).
3. Drop it in this folder and rebuild (`cargo build` / `cargo run`). The build
   script re-runs automatically when this directory changes.

## How it's wired (for reference)

- Embedding + table generation: [`build.rs`](../../build.rs) → `emit_asset_table`.
- Decode + playback by id: [`src/audio.rs`](../../src/audio.rs) (`Sounds::load_all`,
  `Sounds::play`).
- Play sites: `GameApp::play_poke` (per-species poke) and other
  `self.sounds.play("…")` calls in [`src/app.rs`](../../src/app.rs).
