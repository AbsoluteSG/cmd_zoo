# `share/` — copy-paste / QR share codes

This module encodes a small piece of game data into a compact text string (and
QR code) that a player can share, and decodes it back. Two kinds of payload
travel this way:

- **Gift** — "here's an animal for you" (species + level), sent from one player to
  another.
- **Snapshot** — a read-only summary of a zoo (coin count, animal tally, …) for
  showing off, *not* a full save.

A code looks like `czoo1:<base64…>` — the `czoo1:` prefix (`PAYLOAD_PREFIX`)
identifies the format/version so a decoder can reject anything that isn't ours.

## How a code is built

```
Payload (struct)
   │  serde_json  → compact JSON bytes
   ▼
JSON bytes
   │  flate2 (DEFLATE) → compress, because QR codes hold little data
   ▼
compressed bytes
   │  base64 (URL-safe, no padding) → make it copy-paste / URL safe
   ▼
"czoo1:" + base64 text   ──►  also rendered as a QR code (qrcode crate)
```

Decoding runs the same pipeline in reverse: strip the prefix → base64-decode →
decompress → `serde_json` parse back into a `Payload`.

> Why compress? QR codes have a small byte budget and degrade (more error-prone
> to scan) as they fill up. DEFLATE squeezes the JSON so the resulting QR stays
> small and scannable. The snapshot payload deliberately stores a *summary*
> (a per-species tally) rather than every animal, for the same reason.

## Rust / library notes for newcomers

- **`Payload` is an `enum` with a `serde` tag.** `#[serde(tag = "payload_kind")]`
  means the JSON carries a field naming which variant it is (`"gift"` or
  `"snapshot"`), so the decoder knows which shape to expect. This is a common
  serde pattern for "one of several message types."
- **The crates used here are the building blocks, not custom code:**
  `serde`/`serde_json` (serialize), `flate2` (compression), `base64` (text-safe
  encoding), `qrcode` (image generation), `uuid` (ids), `chrono` (timestamps).
- **Everything returns `anyhow::Result`** so a malformed or truncated code fails
  cleanly with a readable error instead of panicking.

## Where it's used

The networking layer (`src/net/`) and the in-game UI call into this module to
produce a code to display and to parse a code a player pastes in. Because the
codec lives in the headless core, the *same* encode/decode runs on every client
and could run server-side too.
