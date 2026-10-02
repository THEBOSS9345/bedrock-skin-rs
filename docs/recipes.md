# Recipes

Worked examples. Each assumes `use bedrock_skin::*;` and a function returning `Result<_, Box<dyn std::error::Error>>`.

## Render a skin file to a PNG

```rust
let texture = decode_image(&std::fs::read("skin.png")?)?;
RenderOptions::new(&texture).render()?.save("body.png")?;
```

## A profile-picture avatar

```rust
let png = RenderOptions::new(&texture).view(View::Avatar).angle(Angle::Iso).size(128).render_png()?;
```

## Render whatever a player is wearing, from a proxy

A Bedrock login carries the skin as base64 fields. The texture is raw RGBA, the geometry is usually the literal `null`, and the resource patch says which model to use.

```rust
use base64::Engine as _;
let b64 = base64::engine::general_purpose::STANDARD;

let texture = texture_from_rgba(b64.decode(&client.skin_data)?, client.skin_image_width, client.skin_image_height)?;
let geometry_raw = b64.decode(&client.skin_geometry_data)?;
let geos = if is_empty(&geometry_raw) { Vec::new() } else { parse_geometry(&geometry_raw)? };
let patch = parse_resource_patch(&b64.decode(&client.skin_resource_patch)?)?;

let png = RenderOptions::new(&texture).geometry(&geos).identifier(patch.default).render_png()?;
```

Take wide vs slim from the patch, not `ArmSize` - see [skin-data.md](skin-data.md#the-resource-patch-is-the-authoritative-model-selector).

## Add a cape

```rust
let cape = texture_from_rgba(cape_pixels, cape_width, cape_height)?;
let img = RenderOptions::new(&texture).cape(&cape).render()?;
```

The cape hangs from the geometry's `cape` bone, or the built-in one when the skin's geometry has none.

## Turntable frames

```rust
let frames: Vec<_> = (0..24)
    .map(|i| RenderOptions::new(&texture).camera(Camera { yaw: i as f64 * 15.0, ..Camera::default() }).size(256).render())
    .collect::<Result<_, _>>()?;
```

## Just the head and one arm

```rust
let img = RenderOptions::new(&texture).parts(["head", "rightArm"]).render()?;
```

## An animated GIF

```rust
let walk = AnimationOptions::new(RenderOptions::new(&texture).size(256), &Motion::Walk).render_gif()?;

let dance = &example_animations()["animation.player.dance"];
let gif = AnimationOptions::new(RenderOptions::new(&texture).size(256), dance).fps(20).render_gif()?;
```

## Handling untrusted uploads

The library sets no limits; what is too large is policy. Bound the three things that cost:

```rust
const MAX_EDGE: u32 = 256;
const MAX_CUBES: usize = 2_000;

let (w, h) = image_dimensions(&upload)?; // header only, no decode
if w > MAX_EDGE || h > MAX_EDGE {
    return Err("skin texture too large".into());
}
let texture = decode_image(&upload)?;

let geos = if is_empty(&geometry_upload) { Vec::new() } else { parse_geometry(&geometry_upload)? };
let (_, cubes) = complexity(&geos);
if cubes > MAX_CUBES {
    return Err("model too complex".into());
}
```

### Bound concurrency too

Each render is single-threaded CPU work, so throughput comes from running several at once - cap that, to bound memory in flight. With tokio, run renders on the blocking pool behind a semaphore:

```rust
let permit = semaphore.acquire().await?;
let png = tokio::task::spawn_blocking(move || {
    let _permit = permit;
    RenderOptions::new(&texture).size(256).render_png()
})
.await??;
```

## Detect an invisible or partly-invisible skin

```rust
let skin = Skin::new(texture, geometry_bytes.as_deref());
let report = skin.report();
match report.verdict {
    Verdict::Invisible => kick(player, "invisible skins are not allowed"),
    Verdict::Suspicious => log::warn!("{} is missing {:?}", player, report.invisible_parts()),
    _ => {}
}
let json = serde_json::to_string(report)?; // for an API or a log
```

Stricter or looser? `Skin::with_options(texture, geometry, SkinOptions { min_visible_parts: 6, ..Default::default() })`.

## Inspecting a model

```rust
let geos = parse_geometry(&raw)?;
for g in &geos {
    println!("{}: {} bones, {} cubes", g.identifier, g.bones.len(), g.total_cubes());
}
let tree = parse_geometry_tree(&raw)?;
for v in tree.select("*/bones/*/pivot") {
    println!("{} = {}", v.path, v.json());
}
```
