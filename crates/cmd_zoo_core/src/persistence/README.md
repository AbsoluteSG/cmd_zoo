# `persistence/` — saving and loading the zoo

This module turns a live `Zoo` (see [`game/`](../game/README.md)) into bytes on
disk and back again. Two jobs:

1. **Serialization** — convert between the runtime `Zoo` and a flat,
   versioned-on-disk shape (`ZooSnapshot`).
2. **Migration** — load *old* save files written by *older* versions of the game
   and upgrade them to today's format, so a player never loses progress when the
   game changes.

## The files

| File | Role |
|------|------|
| `schema.rs` | The on-disk **DTOs** (`ZooSnapshot`, `AnimalDto`, …) and the current `SCHEMA_VERSION`. This is the *wire/file format*, kept separate from the runtime types. |
| `mod.rs` | The conversions (`snapshot_from_zoo`, `zoo_from_snapshot`), the JSON parser, and every `migrate_vN_to_vN+1` function. |
| `json_file.rs` | `JsonFileRepository`: reads/writes the actual file, with cross-process locking and atomic writes. |

## Why a separate "DTO" shape? (`schema.rs`)

The runtime `Zoo` is optimized for gameplay (it uses `HashMap`s, the `glam::Vec2`
math type, `&'static str` species ids, etc.). The on-disk form is optimized for
*stability and serialization*: plain fields, `Vec`s, split-out `x`/`y` floats,
owned `String`s.

> "DTO" = Data Transfer Object — a struct whose only job is to be serialized.
> Keeping it separate means you can refactor the gameplay types freely without
> changing the save format, and vice versa.

Serialization itself is done by **`serde`**, Rust's standard
serialize/deserialize framework. The `#[derive(Serialize, Deserialize)]` on each
DTO auto-generates the JSON conversion — you rarely write it by hand.

The two conversion functions are mirror images:

```
Zoo  ──snapshot_from_zoo──►  ZooSnapshot  ──serde──►  JSON bytes (file)
Zoo  ◄─zoo_from_snapshot──  ZooSnapshot  ◄──serde──  JSON bytes (file)
```

## Migrations: never break an old save

`SCHEMA_VERSION` is currently **19**. Every save file records the version it was
written with. On load, `parse_snapshot` reads that number and walks the file
forward one step at a time until it reaches the current version:

```
v1 → v2 → v3 → … → v19   (each arrow is one migrate_vN_to_vN+1 function)
```

Each `migrate_vN_to_vN+1` does the *smallest possible* JSON edit to bring the
file up a version — usually "add this new field with a sensible default." For
example, when DNA Helix was added (v8→v9) the migration seeds it to 0; when the
procedural world arrived (v12→v13) it derives a stable `world_seed` from the
player id so existing zoos get a reproducible world.

> These functions operate on **`serde_json::Value`** — an untyped, in-memory JSON
> tree — *before* the data is parsed into the strict typed `ZooSnapshot`. That's
> deliberate: an old file is missing fields the typed struct requires, so you
> patch the loose JSON first, then do the strict typed parse once it's current.

**If you add or change a saved field, you must:** bump `SCHEMA_VERSION`, add a new
`migrate_vN_to_vN+1`, wire it into the `match` in `parse_snapshot`, and (ideally)
add a test that an old hand-written save migrates correctly. The tests at the
bottom of `mod.rs` show the pattern.

### Tolerant loading

`zoo_from_snapshot` is forgiving on purpose. If a save references a species or
structure kind that no longer exists in the catalog (e.g. it was renamed), that
entry is **dropped and recorded in a `warnings` list** rather than making the
whole save unloadable. The app surfaces those warnings as a status banner — so
"what happened to my frog?" stays answerable. Genuinely unrecoverable problems
(unknown schema version, unknown habitat theme) are still hard errors.

## The file on disk (`json_file.rs`)

`JsonFileRepository` owns the save file in the platform's standard app-data
directory (via the `directories` crate). Two robustness details worth knowing:

- **Atomic writes** — it writes to a temp file and renames it into place, so a
  crash mid-write can't corrupt your existing save (a rename is atomic on the
  filesystem; a partial write would not be).
- **File locking** — it uses an OS file lock (`fs2`) so two running copies of the
  game can't clobber each other's saves. The lock is held across the
  load→mutate→save critical section.

> Rust note: `ZooRepository` (in `mod.rs`) is a **trait** — an interface. Today
> the only implementor is `JsonFileRepository`, but the trait is the seam where a
> different backend (e.g. a SpacetimeDB-backed repository) could plug in without
> the rest of the game noticing.

## Tests

The bottom of `mod.rs` has the meaningful tests: round-trips (`Zoo` → JSON →
`Zoo` preserves everything) and migrations (a minimal old-version save upgrades
to the current schema). Run them with:

```sh
cargo test -p cmd_zoo_core persistence
```
