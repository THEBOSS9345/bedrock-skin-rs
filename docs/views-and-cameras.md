# Views, bone scoping and cameras

> This page is shared with [bedrock-skin-go](https://github.com/THEBOSS9345/bedrock-skin-go): the format and the pipeline are the same in both. The Rust API has the same names in Rust style - `ParseGeometry` is `parse_geometry`, `Options` is `RenderOptions`, `ViewAvatar` is `View::Avatar`, `Options.Geometry` is `RenderOptions::geometry` - and [api-reference.md](api-reference.md) lists them all.

Two independent questions decide what a render looks like: **which bones are drawn**, and **where the camera sits**. `View` answers the first (or `Parts` does), `Angle` answers the second (or `Camera` does).

## Bone scoping is ancestry-based

The key idea, and the reason custom skins work with no special-casing: a view never lists the bones it wants. It names an **anchor**, and every bone whose parent chain reaches that anchor is included.

```rust
fn is_descendant(by_name: &HashMap<&str, &Bone>, name: &str, ancestor: &str) -> bool {
    let mut seen = Vec::new();
    let mut cur = name;
    while !cur.is_empty() && !seen.contains(&cur) {
        if cur == ancestor {
            return true;
        }
        seen.push(cur);
        cur = by_name.get(cur).map_or("", |b| b.parent.as_str());
    }
    false
}
```

So `ViewHead` asks for "everything descended from `head`". On a vanilla skin that is `head` and `hat`. On a custom skin it is also the ears, the horns, the party hat, the halo — whatever the creator parented there, without this library knowing any of those names.

That is why there is no hardcoded bone list anywhere in the render path. The `seen` set guards against a malformed file whose parent chain loops.

## The four views

| View | Includes | Framing |
| --- | --- | --- |
| `ViewAvatar` | Everything under `head` | Tight square icon — FOV 25, margin 1.15 |
| `ViewHead` | Everything under `head` | Same bones, more headroom — FOV 30, margin 1.4 |
| `ViewChest` | Under `head`, `leftArm`, `rightArm`, plus `body` and `waist` | FOV 35, margin 1.5 |
| `ViewBody` | Everything | FOV 35, margin 1.6 |

`ViewAvatar` and `ViewHead` differ only in framing — same bones, different crop.

`ViewChest` deliberately includes the arms with their own descendants (sleeves, held-item locators, arm-mounted custom bones), so it reads as a bust rather than a floating torso.

`ViewBody` includes everything by using a `nil` filter, which skips the ancestry walk entirely.

The FOV and margin per view come from `framingFor`. Head-scoped views use a narrower field of view because perspective distortion is much more obvious on a face.

## Explicit parts

`Parts` overrides `View` and takes bone names directly:

```rust
let img = RenderOptions::new(&texture).parts(["head", "leftArm", "rightArm"]).render()?;
```

Each name pulls in its descendants, exactly like a view anchor — naming `head` still brings the hat. An empty list means everything.

`ParseParts` exists for the common case of accepting that list as a comma-separated string from an HTTP form or CLI flag:

```rust
let parts = bedrock_skin::parse_parts("head, leftArm, rightArm");
```

It trims whitespace and drops empty entries, so `"head,,leftArm,"` behaves sensibly.

When `Parts` is set, framing falls back to the generic FOV 35 / margin 1.5 rather than a per-view preset — there is no view to look up.

A parts list matching no bones is an **error**, not an empty image. Silently returning a blank picture hides a typo.

## Angles

Two presets:

| Angle | Yaw | Pitch | Look |
| --- | --- | --- | --- |
| `AngleFront` | 0° | 0° | Straight-on portrait |
| `AngleIso` | 35° | 25° | Three faces at once — the classic head-icon look |

`AngleIso` also multiplies margin by 1.25, because the diagonal camera is offset rather than pulled straight back and would otherwise clip a corner.

The default when `Angle` is unset comes from `defaultAngleFor`: **`ViewHead` defaults to iso**, everything else to front. A plain front portrait of a head hides the top, and the top is exactly where custom bones tend to live — a hat, ears, horns. For a full figure or a bust, straight-on reads better than an angled shot.

The iso values of 35°/25° were chosen to show front, top and one side without foreshortening any of them into invisibility.

## Explicit cameras

`Camera` overrides `Angle` entirely:

```rust
let img = RenderOptions::new(&texture)
    .camera(Camera { yaw: 35.0, pitch: 15.0, fov: 30.0, margin: 1.4 })
    .render()?;
```

| Field | Effect | Zero means |
| --- | --- | --- |
| `Yaw` | Rotation around vertical. Positive swings toward the model's left. | 0 (front) |
| `Pitch` | Elevation. Positive looks down. | 0 (level) |
| `FOV` | Field of view in degrees. | 35, or the view's preset |
| `Margin` | Framing room. 1.0 is as tight as possible. | 1.5, or the view's preset |

`FOV` and `Margin` are only overridden when non-zero, so you can set just a yaw and keep a view's framing:

```rust
// View::Avatar's tight framing, but seen from the side.
RenderOptions::new(&texture)
    .view(View::Avatar)
    .camera(Camera { yaw: 60.0, ..Camera::default() })
```

### Choosing a FOV

FOV changes the *character* of the shot, not just the zoom, because the camera distance is recomputed to keep the subject the same size:

- **15–20°** — nearly orthographic. Flat, technical, good for sprite sheets.
- **25–35°** — natural. The presets live here.
- **50°+** — noticeable perspective distortion. Dramatic for close head shots, unflattering for full bodies.

### Choosing a margin

- **1.0** — as tight as the bounding box allows.
- **1.15–1.6** — the preset range.
- **2.0+** — small subject in a lot of empty space, useful when compositing onto a background.

Because framing is derived from the actual triangle bounding box, these behave consistently across models of wildly different sizes. A skin with enormous wings frames itself correctly at the same margin as a vanilla body.
