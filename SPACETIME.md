# SpacetimeDB — command cheat sheet (cmd_zoo)

Everything you need to build, deploy, regenerate, and inspect the online backend
(`crates/cmd_zoo_stdb`). Run all of these **from the repo root**
(`Z:\Programming\RUST\cmd_zoo`).

- **CLI:** `spacetime` (installed at `~/AppData/Local/SpacetimeDB/spacetime`).
- **Module crate:** `crates/cmd_zoo_stdb` (the Rust → wasm authority module).
- **Client bindings:** `src/stdb/bindings` (generated; never hand-edit).
- **Live database name:** `critter-cove` on **Maincloud**
  (`-s maincloud`). Local dev DB is `cmd-zoo-dev` (`-s local`).

> The single rule that prevents 90% of "it broke" confusion:
> **whenever you change anything under `crates/cmd_zoo_stdb/`, republish before
> testing.** If the change alters table or reducer *shapes* (not just logic),
> also regenerate bindings and rebuild the client.

---

## The everyday loop

### 1. Republish the module — after ANY module change
```
spacetime publish -s maincloud -p crates/cmd_zoo_stdb critter-cove
```
- Builds the module and deploys it to Maincloud.
- `-p crates/cmd_zoo_stdb` is the **module path** — **required**. (Without it the
  publish lint scans the whole client tree and fails on the client's `eprintln!`s.)
- It prompts `publish to a non-local server? [y/N]` → press **y**.
- **Additive changes** (new tables, new reducers, new columns) migrate in place
  and **keep all existing data** — no reset needed.
- Only **incompatible changes** (dropping/renaming/retyping a column, changing a
  primary key) require a destructive migration — see *Resetting* below.

### 2. Regenerate client bindings — only when table/reducer SHAPES changed
```
spacetime generate --lang rust --module-path crates/cmd_zoo_stdb --out-dir src/stdb/bindings
```
- Rewrites `src/stdb/bindings/*` so the client knows the new tables/reducers.
- Needed when you **add/remove/rename a table, reducer, or column** — not for
  pure logic changes inside an existing reducer.
- After regenerating, update `src/stdb/client.rs` (subscriptions + reducer
  wrappers) to use the new shapes, then rebuild the client (`cargo build`).

### 3. Rebuild + run the client
```
cargo build          # close any running game first — the .exe locks
cargo run
```
Then press **F7** in-game to connect to the hub.

> **Mismatch symptom to remember:** the client subscribes to a fixed set of
> tables. If the deployed module is missing any of them (e.g. you rebuilt the
> client but forgot to republish), the **entire** subscription fails and you see
> *no* peers and *no* shared world — looks like a "seed bug." Fix = republish.

---

## Inspecting live state (debugging)

### Run SQL against the database
```
spacetime sql -s maincloud critter-cove "SELECT * FROM account"
spacetime sql -s maincloud critter-cove "SELECT owner, slot, coins FROM zoo"
spacetime sql -s maincloud critter-cove "SELECT * FROM party_member"
```
- Read-only-ish ad-hoc queries over the public tables. Great for confirming a
  reducer did what you expected, or whether a table exists at all.
- An `Error: no such table` means that table isn't deployed → republish.

### Tail the module's logs (reducer `log::info!` output)
```
spacetime logs -s maincloud critter-cove
spacetime logs -s maincloud critter-cove -f      # follow (live tail)
```
- Shows connect/disconnect, `join_hub`, party events, `action rejected: …`, etc.
- The first place to look when a reducer call "does nothing."

### Describe the deployed schema
```
spacetime describe -s maincloud critter-cove
```
- Dumps the live tables + reducers. Use to verify a publish actually landed.

### Call a reducer manually (without the game client)
```
spacetime call -s maincloud critter-cove move_avatar 250000 250000
spacetime call -s maincloud critter-cove leave_party
```
- Invokes a reducer as your CLI identity. Handy for poking server logic directly.
- Args are space-separated, in the reducer's parameter order.

---

## Resetting / wiping data (use sparingly)

You do **not** need this for normal testing. Reach for it only when:
- a publish refuses because of an **incompatible** schema change, or
- you deliberately want a clean slate (e.g. clear out test accounts/zoos).

```
# Publish AND accept a destructive migration (drops data as needed):
spacetime publish -s maincloud -p crates/cmd_zoo_stdb -c critter-cove

# Delete the database entirely (then a fresh publish recreates it):
spacetime delete -s maincloud critter-cove
```
- `-c` / `--clear-database`: wipe table data so an incompatible schema can apply.
- `--yes=migrate` / `--yes=break-clients`: skip the corresponding safety prompts
  non-interactively (know what you're doing — these bypass "this will BREAK
  existing clients" warnings).

---

## Identity / auth (occasional)
```
spacetime login                       # log in to your SpacetimeDB account
spacetime logout
spacetime server list                 # see configured servers (local, maincloud)
```
The game client connects **anonymously** today (the SDK mints an identity on
first connect); Steam-linked stable identity comes later.

---

## Quick reference

| You changed…                                  | Run                                    |
|-----------------------------------------------|----------------------------------------|
| Reducer **logic** only (same tables/sigs)     | `publish`                              |
| Added/removed/renamed a **table or reducer**  | `publish` → `generate` → `cargo build` |
| Added/removed/retyped a **column**            | `publish` (maybe `-c`) → `generate` → `cargo build` |
| Nothing on the server, just the **client**    | `cargo build` (no publish)             |
| "Can't see anyone after F7"                   | check `sql`/`describe`, then `publish` |
| Want to see what the server is doing          | `spacetime logs -f`                    |

Server: `-s maincloud` (live `critter-cove`) · `-s local` (dev `cmd-zoo-dev`).
