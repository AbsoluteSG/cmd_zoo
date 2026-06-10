# `render/` — the Macroquad presentation layer

Everything visual lives here: how the tile world is projected to the screen, how
sprites are loaded and cached, how the world scene / HUD / menus are drawn, and
the decorative layers (grass, terrain props, particles). 

**This folder draws; it never decides.** All game rules are in
[`cmd_zoo_core`](../../crates/cmd_zoo_core/README.md). These modules read game
state and produce pixels. That one-way dependency is the whole point — you can
change the look without touching the rules, and the rules can run headless
(server-side) without any of this.

## Immediate-mode drawing (important mental model)

Macroquad uses **immediate-mode** rendering: there is no retained scene graph or
widget tree. Every frame you call functions like `draw_texture`, `draw_rectangle`,
`draw_text` in order, and what you draw is what you see *this frame only*. A
button isn't an object that exists between frames — it's "draw a rectangle, then
check if the cursor is inside it and the mouse was clicked." If you're coming
from React/DOM or a retained UI toolkit, this is the big adjustment.

Because there's no z-buffer for our sprites, **depth is the painter's algorithm**:
things are sorted by their screen-Y and drawn back-to-front, so a critter lower on
the screen correctly overlaps one behind it.

## The files

### Projection & cache (the foundation)
- **`view.rs`** — the 2.5D projection math. The world is a flat tile grid drawn as
  if seen from a camera tilted ~55° down: `screen = (tx·TILE_W, ty·ROW_H)·zoom +
  offset`. Crucially `screen_to_tile` is the *exact inverse*, so clicking a tile to
  place something hit-tests correctly.
- **`textures.rs`** — the texture cache. PNGs are embedded into the binary by
  `build.rs`; each is decoded into a `Texture2D` on first use and cached by id.
  Lookup is fuzzy (snake_case file ↔ camelCase id), and a missing id returns
  `None` so callers can fall back to placeholder drawing.

### The scene
- **`world.rs`** — the main world scene: the ground plane plus roaming critter
  sprites as upright billboards with drop shadows, depth-sorted with avatars and
  terrain props.
- **`mod.rs`** — module root; ties the submodules together.

### Decorative layers (cosmetic, deterministic)
- **`grass.rs`** — a field of waving grass blades built as a per-frame triangle
  mesh (no textures): each blade has a colour gradient and a wind-swayed tip.
- **`terrain.rs`** — scattered non-interactive props (rocks, plants, trees) from
  `assets/terrain/<Biome>/*.png`, depth-sorted with critters so a tree can sit in
  front of or behind a passing animal.
- **`particles.rs`** — a texture-free, object-pooled particle system (particles are
  small filled rects). The pool is a fixed-capacity `Vec` reused round-robin, so a
  burst can never allocate unbounded.

> "Deterministic" here means placement is seeded from the zoo's `world_seed`, so
> the grass and props look identical every frame and across sessions — they're
> *generated*, not randomly re-rolled.

### UI
- **`ui.rs`** — shared UI primitives + the colour palette (rounded rects, fades,
  easing). One vocabulary so the HUD and menus look consistent.
- **`menus.rs`** — the Shop / Breeding overlays: a panel that scales/fades in over
  the blurred, darkened world. Buttons are immediate-mode: hit-test the cursor
  inline and call a domain method.

## Where shaders / post-processing live

The fullscreen post-process shaders (Scanlines, Pixelate, Grayscale, Sepia) and
the menu blur are applied in `GameApp::draw` (`src/app.rs`): the world is rendered
into an offscreen render target, then drawn to the screen through a shader. The
particle system is safe to call from inside that offscreen pass because it draws
only primitives, never text.

## If you're adding art or a new visual

- **New animal/tile/habitat sprite:** drop the PNG into the right `assets/`
  subfolder; `build.rs` embeds it and `textures.rs` will find it by id (fuzzy
  match). No code change needed for the texture itself.
- **New menu:** copy the pattern in `menus.rs` — draw the panel with `ui.rs`
  helpers, then hit-test buttons and call into `crate::game`.
- **Keep rules out of here.** If you find yourself computing a price or deciding
  whether an action is allowed in `render/`, that logic belongs in
  `cmd_zoo_core` instead; call into it and just draw the result.
