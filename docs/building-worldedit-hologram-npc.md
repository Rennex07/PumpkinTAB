# Building WorldEdit, Hologram and NPC plugins on Pumpkin

What the Pumpkin plugin API actually gives you, per plugin, with the WIT calls each
one needs. Everything here was read out of `crates/pumpkin-plugin-wit/v0.1` at
commit `204a94ed` (tag `0.2.0+26.3-26.51`) — not from docs, because the docs
don't cover this yet.

**Short version:** all three are buildable. WorldEdit is the most work. NPC needed a
second look to confirm.

---

## Read this first: the API is a moving target

Two things will shape every decision below.

**A plugin only loads against a byte-identical WIT tree.** Not a semver range — the
actual file contents. Pumpkin kept the version string `0.2.0+26.3-26.51` across a
WIT change, so the version cannot warn you. Get it wrong and you get:

```
ERROR Failed to load plugin from "./plugins\my_plugin.wasm": Wasm plugin
initialization error: Plugin is built against a different API version:
type-checking export func `handle-event`
```

Pin your build to a commit and record the tree hash:

```powershell
git rev-parse HEAD:crates/pumpkin-plugin-wit/v0.1
```

**There is no bulk operation.** Every block edit is one IPC call. `set-block` takes a
single `block-pos`. There is no `fill`, no `set-blocks`, no region primitive. A
100×100×100 selection is a million round-trips. This is the single biggest
constraint on WorldEdit and it's worth designing around from line one.

---

## WorldEdit

### What the API gives you

Block mutation lives on the **world** resource (`world.wit`):

```wit
set-block:          func(pos: block-pos, block: block, update-flags: block-flags);
set-block-by-id:    func(pos: block-pos, block-id: u16, update-flags: block-flags);
set-block-by-name:  func(pos: block-pos, name: string, update-flags: block-flags) -> bool;
set-block-state:    func(pos: block-pos, state: u16, update-flags: block-flags);
get-block:          func(pos: block-pos) -> block;
get-block-id:       func(pos: block-pos) -> u16;
get-block-state:    func(pos: block-pos) -> block-state;
set-block-entity-nbt: func(pos: block-pos, nbt-data: list<u8>) -> result<_, string>;
get-block-entity-nbt: func(pos: block-pos) -> option<list<u8>>;
```

Name lookup is available and is what makes a config-driven plugin pleasant:

```wit
get-block-by-name: func(name: string) -> option<block>;
get-states-for-block: func(block: block) -> list<block-state>;
get-default-state-from-block: func(block: block) -> block-state;
```

`set-block-by-name` returning `bool` means you can resolve a name per-edit without a
lookup table. **Cache it anyway** — resolving once and reusing the `block` value is
strictly cheaper.

There is a second, chunk-relative surface on the `chunk` resource, with the same
getters/setters taking chunk-local coordinates (`u8` x/z, `s32` y). Use it to avoid
a `get-chunk` lookup per block:

```wit
resource chunk {
    get-block-state-id: func(pos: block-pos) -> u16;
    set-block-state:    func(pos: block-pos, state: u16);
    get-height:         func() -> u32;
}
```

Persistence is explicit. Nothing you change is on disk until you say so:

```wit
save: func() -> result<_, string>;   // "Saves all chunk data, block entities, and entities"
```

### `block-flags` is where the semantics live

```wit
flags block-flags {
    notify-neighbors,                            // tell adjacent blocks
    notify-listeners,                            // tell clients
    force-state,                                 // write even if unchanged
    skip-drops,                                  // no item drops on removal
    moved,                                       // piston-style move
    skip-redstone-wire-state-replacement,
    skip-block-entity-replaced-callback,
    skip-block-added-callback,
}
```

The combination that matters most:

| Operation | Flags |
|---|---|
| Paste (silent, instant) | `notify-neighbors` only |
| Paste (correct, slower) | `notify-neighbors, notify-listeners` |
| Replace (no drops) | `notify-neighbors, notify-listeners, skip-drops` |
| Pure clipboard restore | `force-state, skip-drops` |

`skip-drops` is what stops a `//set stone` from showering players with cobblestone.
Getting this wrong is the most common way a WorldEdit clone feels broken.

### Commands

`command.wit`'s `argument-type` covers what a wand/selection plugin needs:

```wit
block-pos, position3d, position2d, column-pos,
entities, entity, players, block-state, block-predicate,
nbt-compound-tag, nbt-tag, nbt-path, int-range, ...
```

`block-pos` and `position3d` are what `//pos1`, `//pos2` and `//wand` take.
`nbt-compound-tag` means you can accept a raw NBT snippet as an argument, which is
how `//set` with full block state works without inventing your own parser.

`register-command` takes a `command` and a permission string — one permission per
command, no permission *tree*. Plan your node layout accordingly.

### What you have to build

Everything above the primitive. In rough dependency order:

1. **Selection model** — cuboid first (`pos1`/`pos2`). Then poly (wand drag) and
   ellipsoid. Store as a region enum, not three unrelated fields.
2. **Clipboard** — serialize to `Vec<u8>` and write through your data folder
   (`fs.write.data`). Include world + dimension, or pasting across worlds silently
   corrupts.
3. **History** — undo/redo stacks of `(pos, previous-state-id)`. This is the memory
   hog: a million-block edit is a million entries. Cap it and make the cap explicit.
4. **Batching** — walk the selection in chunk-sized slabs so you resolve each chunk
   once. Without this, a large edit is unusable rather than slow.
5. **Persistence** — call `save` when you want it on disk, not per block.

**Verdict: buildable, real work.** The API can express every operation WorldEdit
does. Performance is entirely on you, and per-block IPC means you must batch or it
will not feel like WorldEdit.

---

## NPC

This one I initially got wrong — I reported no spawn call, then found it. It exists:

```wit
// world.wit:900
/// Spawns an entity of the specified type at the given position.
spawn-entity: func(entity-type: entity-type, pos: position) -> entity;
```

It **returns the entity handle**, so you spawn and configure in one step. `entity-type`
is a 161-variant enum, so any vanilla mob is reachable by name.

On the returned entity:

```wit
get-uuid:          func() -> uuid;                  // for your persistence key
get-type:          func() -> entity-type;
get-position:      func() -> position;
teleport:          func(pos: position, world-ref: %world);
set-velocity:      func(velocity: position);
set-custom-name:   func(name: text-component);      // <- the NPC name
set-custom-name-visible: func(visible: bool);
set-health:        func(health: f32);
set-silent:        func(silent: bool);
remove:            func();
```

`set-custom-name` takes a `text-component`, so an NPC name is full MiniMessage —
colours, bold, hover, all of it.

Enumerate what's there:

```wit
get-entities: func() -> list<entity>;
```

### Persistence via custom data

This is the good part. Entities carry namespaced NBT, same as chunks and worlds:

```wit
set-custom-data: func(namespace: string, key: string, value: nbt-tree);
get-custom-data: func(namespace: string, key: string) -> option<nbt-tree>;
remove-custom-data: func(namespace: string, key: string);
```

So tag each NPC:

```
namespace: "myplugin", key: "npc", value: { skin: "villager", path: [...] }
```

and your plugin becomes invisible to other plugins and survives restarts without you
owning a UUID database. You can drive the same three calls on the world and chunk
resources for plugin-scoped world data.

### Interaction

```wit
player-interact-entity-event-data
```

is in the event list. That's your right-click handler — the hook Citizens uses.
Combine with `entity-teleport-event-data` for `/npc gohere` style following.

`player-interact-unknown-entity-event-data` also exists, which suggests a path for
entities the server doesn't otherwise recognise.

### Verdict

**Buildable, and the cheapest of the three.** Spawn, name, persist, react to clicks.
The whole plugin is a few hundred lines. The gap versus Citizens is pathfinding and
player-model skins, neither of which the API exposes.

---

## Hologram

Fully supported. `display.wit` is a complete text-display resource.

```wit
resource text-display-entity {
    from-entity: static func(entity: borrow<entity>) -> option<text-display-entity>;
    get-entity:   func() -> entity;

    get-text:     func() -> text-component;
    set-text:     func(text: text-component);

    set-line-width:    func(width: s32);      // wrap width
    set-background:    func(color: s32);      // ARGB
    set-text-opacity:  func(opacity: s8);
    set-shadow:        func(shadow: bool);
    set-see-through:   func(see-through: bool);
    set-default-background: func(default-background: bool);
    set-alignment:     func(alignment: text-alignment);
}

resource display-entity {          // shared base
    set-transformation: func(transformation: display-transformation);
    set-billboard:      func(mode: billboard-mode);
    set-interpolation-duration: func(duration: s32);
    set-teleport-duration:      func(duration: s32);
    set-view-range:    func(range: f32);
    set-brightness:    func(brightness: s32);
    set-glow-color-override: func(color: s32);
}
```

Enums:

```wit
enum billboard-mode  { fixed, vertical, horizontal, center }
enum text-alignment  { center, left, right }
enum item-display-mode { none, thirdperson-lefthand, ..., gui, ground, fixed }
```

Transforms are full 3D — translation, scale, and **quaternions** for both rotations:

```wit
record display-transformation {
    translation: vector3f,
    scale: vector3f,
    left-rotation: quaternionf,
    right-rotation: quaternionf,
}
```

That covers spinning (`left-rotation` animated per tick) and floating offsets.

### How to get one

`text-display-entity` has `from-entity: static func(entity: borrow<entity>)` — it's
obtained **from an existing entity**, not constructed. So the flow is:

1. `spawn-entity(entity-type, pos)` → the handle
2. `text-display-entity::from-entity(&handle)` → `option<text-display-entity>`

The `option` is your feature check: `None` means the server didn't give you a display
entity, so degrade rather than crash. Worth handling, since you're pinned to one
server build but users update.

### Persistence

Same custom-data trick as NPCs — tag it, reload on start.

### Verdict

**Buildable, and the least work of the three.** This is the most complete part of the
API for a real plugin. Everything DecentHolograms does is a direct call. The one
structural oddity is the `from-entity` two-step, and you must handle the `None`.

---

## Skript

No scripting runtime exists, and none is implied. Every `.wasm` plugin is compiled
Rust.

The realistic middle path is what PumpkinTAB already does: a **declarative config
interpreted by compiled Rust**. Concretely, the pattern already in use:

- `animations:` — named, timed text frames. A tiny state machine.
- `conditions:` — named boolean tests with comparators (`>=`, `<`, `=`, `|-`).
- `<text>||<number format>||<score>` — positional mini-syntax with an explicit split.

Each is a small embedded DSL with no filesystem and no IPC. Extend that shape rather
than building a language.

**If you do want user-authored scripting**, the trust model is the thing to get right.
In Bukkit, Skript runs arbitrary Java and the admin already trusts the server. Here,
each script is a `wasm32-wasip2` component requesting host permissions —
`fs.read.data`, `fs.write.data` are the ones your plugins already request. An engine
that runs uploaded code hands out filesystem access.

The safer design is a constrained DSL: no filesystem, no IPC, an allowlist of
effects, and resource limits. A bad script can then break the server without touching
the machine. That is a meaningfully different product from Skript, and worth deciding
deliberately rather than discovering later.

---

## Summary

| Plugin | Verdict | Main cost |
|---|---|---|
| **WorldEdit** | Buildable | Batching. No bulk API — per-block IPC. Undo history is memory-hungry. |
| **NPC** | Buildable | Least work. Spawn + name + custom-data persistence + interact event. |
| **Hologram** | Buildable | Least risk. API is complete; only the `from-entity` two-step and its `None` case. |
| **Skript** | No runtime | Build a constrained DSL if you want scripts. Don't expose a compiler. |

## Open questions worth answering before you scope

1. **Do packets expose anything client-side?** `packet-sent-event-data` and
   `packet-received-event-data` are in the event list. If they can be intercepted or
   injected, a lot of NPC and hologram tricks open up that the display API doesn't
   cover. I did not check this.

2. **Is there an entity-spawn cost limit?** `spawn-entity` has no throttle parameter.
   A server spawning hundreds of NPCs may have a problem the API doesn't warn you
   about.

3. **Does `set-block-by-name` fail loudly on a typo?** It returns `bool`. Presumably
   `false`, but worth confirming whether it's a silent no-op or logs — this decides
   whether a bad config name is visible to an admin.

## Provenance

Read from `crates/pumpkin-plugin-wit/v0.1` at commit `204a94ed`
(tag `0.2.0+26.3-26.51`), tree `e0bb97ea`. Line numbers are from that tree.

If you build against a different commit, re-verify: `spawn-entity` is the call most
likely to have moved, and its presence is what makes NPCs viable at all.
