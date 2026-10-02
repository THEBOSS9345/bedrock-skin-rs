# bedrock-skin

**Render Minecraft Bedrock skins to PNG and GIF, in pure Rust.** 3D bodies, heads and avatars, capes, slim and wide arms, custom geometry, persona skins, animations from Blockbench files, and a detector for invisible skins.

<p align="center">
  <img src="https://raw.githubusercontent.com/THEBOSS9345/bedrock-skin-rs/main/docs/images/body-front.png" width="160" alt="A skin rendered full body, front on">
  <img src="https://raw.githubusercontent.com/THEBOSS9345/bedrock-skin-rs/main/docs/images/body-iso.png" width="160" alt="The same skin from an angle">
  <img src="https://raw.githubusercontent.com/THEBOSS9345/bedrock-skin-rs/main/docs/images/avatar.png" width="160" alt="The skin's head as an avatar">
  <img src="https://raw.githubusercontent.com/THEBOSS9345/bedrock-skin-rs/main/docs/images/walk.gif" width="160" alt="The skin walking">
  <img src="https://raw.githubusercontent.com/THEBOSS9345/bedrock-skin-rs/main/docs/images/dance.gif" width="160" alt="The skin dancing">
</p>

Texture in, image out. No GPU, no headless browser, no C library: a small software rasterizer, `#![forbid(unsafe_code)]`. It reads skins the way a Bedrock (MCPE) client sends them, so it drops straight into a proxy, a server, a Discord bot or a website.

```bash
cargo add bedrock-skin
```

It is the Rust version of [bedrock-skin-go](https://github.com/THEBOSS9345/bedrock-skin-go), and the two render **the same images, pixel for pixel** - the tests check every render, animation frame, pose and report against the Go library's output. It is also about three times faster.

## Quick start

```rust
use bedrock_skin::{RenderOptions, View, Angle};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let texture = bedrock_skin::decode_image(&std::fs::read("skin.png")?)?;

    // A full body, straight on, 512x512.
    RenderOptions::new(&texture).render()?.save("body.png")?;

    // A 256px head icon from an angle, as PNG bytes.
    let png = RenderOptions::new(&texture).view(View::Avatar).angle(Angle::Iso).size(256).render_png()?;
    std::fs::write("avatar.png", png)?;
    Ok(())
}
```

Skins coming off the wire arrive as raw RGBA rather than an encoded image; `texture_from_rgba` wraps those. Holding encoded bytes and wanting bytes back? `render_bytes` folds the decode and encode in.

## Geometry is optional, and that matters

A Bedrock client sends **no mesh at all** for a skin that uses one of the built-in models: its login packet carries the literal JSON `null`, and names the model only in the skin's resource patch. So leaving `geometry` empty is not a shortcut - for most real skins it is the correct input, and the vanilla humanoid stands in.

When a skin does carry geometry, parse it, and pick the entry its resource patch names:

```rust
let geos = bedrock_skin::parse_geometry(&geometry_bytes)?;
let patch = bedrock_skin::parse_resource_patch(&resource_patch_bytes)?;
let img = RenderOptions::new(&texture)
    .geometry(&geos)
    .identifier(patch.default) // e.g. "geometry.humanoid.customSlim"
    .render()?;
```

`parse_geometry` reads both of Bedrock's formats, the modern `minecraft:geometry` array and the pre-1.12 one. Take wide vs slim from the resource patch, not the login packet's `ArmSize`: real captures show the two disagreeing.

## Options

| Field | Meaning |
| --- | --- |
| `texture` | The skin image. The only required field. |
| `geometry` | From `parse_geometry`. Empty uses `default_geometry()`. |
| `identifier` | Which entry to render. Empty picks the one with the most cubes. |
| `cape` | A cape texture, drawn from the geometry's `cape` bone or the built-in one. Not drawn for head and avatar views. |
| `view` | `Body`, `Chest`, `Head` or `Avatar`. |
| `angle` | `Front` or `Iso`; None is the view's default. |
| `parts` | Exact bone names, e.g. `["head", "leftArm"]`. Overrides `view`. |
| `camera` | Explicit yaw, pitch, FOV and margin. Overrides `angle`. |
| `size` | Output edge length; 0 means 512. Always square. |
| `pose` | Moves bones, e.g. one frame of an animation. |

Bones are picked by ancestry, so naming `head` also brings a hat, hair, ears or horns parented under it. Custom skins work with no special-casing.

`parse_view`, `parse_angle` and `parse_parts` turn request parameters into options, and **reject** names they don't know, so a request for `avatr` is an error rather than a full-body render.

## Animation

Minecraft's own player motions are built in (`Motion::Walk`, `Idle`, `Wave`, `Sneak`), and `parse_animations` reads any Bedrock animation file - what Blockbench exports - with keyframes, smooth interpolation and Molang expressions.

```rust
use bedrock_skin::{AnimationOptions, Motion};

let gif = AnimationOptions::new(RenderOptions::new(&texture).size(256), &Motion::Walk).render_gif()?;

let anims = bedrock_skin::parse_animations(&blockbench_export)?;
let frames = AnimationOptions::new(RenderOptions::new(&texture), &anims["animation.player.wave"]).render_frames()?;
```

33 example animations come bundled - dances, emotes, a backflip, fighting moves - as `example_animations()` and as files in [examples/animations](examples/animations). Not every Minecraft animation plays on every model: an animation moves bones by name, so one made for a mob with wings does nothing on a player. `missing_bones` tells you.

## Reading geometry files

`parse_geometry_tree` keeps a whole geometry file and picks any value out of it by path:

```rust
let tree = bedrock_skin::parse_geometry_tree(&raw)?;
let pivot = tree.get("geometry.humanoid.custom/bones/rightArm/pivot");   // [-5, 22, 0]
let sizes = tree.select("*/bones/*/cubes/*/size");                       // every cube's size
```

## Persona skins

Persona (character creator) skins are built from poly meshes instead of cubes, and render in 3D like any other model. Their parts are spread over several geometry entries, and the head is textured by the skin's face animation rather than the skin image - add the animation images to draw it:

```rust
let img = RenderOptions::new(&tex)
    .geometry(&geos)
    .animated(AnimatedType::Face, &face)
    .render()?;
```

Without them the body renders and the head view returns `Error::EmptyView`. See [docs/geometry-format.md](docs/geometry-format.md#persona-skins).

## Detecting invisible skins

The same inputs feed a detector for the "invisible player" trick:

```rust
use bedrock_skin::{Skin, Verdict};

let skin = Skin::new(texture, geometry_bytes.as_deref());
match skin.report().verdict {
    Verdict::Invisible => { /* nothing renders, or only a stray limb */ }
    Verdict::Suspicious => { /* some body parts missing - worth a look */ }
    _ => {}
}
```

The report has a verdict, how many of the six standard parts render, and a per-part breakdown, and it serializes straight to JSON (serde) in the same shape as the Go version's. With geometry, it checks the texture where the cubes actually map, so transparent regions and too-tiny bones are caught; a cape never masks an invisible body; persona skins are trusted. `SkinOptions` sets the thresholds.

## Untrusted input

The library sets no limits of its own - what is too large is your policy. For arbitrary uploads:

- Check `image_dimensions` before decoding: it reads only the header, and a few-KB PNG can declare enormous dimensions.
- Check `complexity` (bones and cubes) before rendering a geometry document.
- Bound how many renders run at once. Each is single-threaded CPU work.

## Try it

```bash
cargo run --example render -- skin.png avatar.png avatar iso 256
cargo run --example render -- skin.png dance.gif dance
```

## Documentation

API docs are on [docs.rs](https://docs.rs/bedrock-skin). [`docs/`](docs/) explains how Bedrock skins work and how this library draws them: [what a client sends](docs/skin-data.md), [the geometry format](docs/geometry-format.md), [the rendering pipeline](docs/rendering-pipeline.md), [views and cameras](docs/views-and-cameras.md), [animation](docs/animation.md), [recipes](docs/recipes.md) and [why it works the way it does](docs/design-decisions.md).

## Contributing

Pull requests welcome, and so is using AI to write them - point it at [`docs/`](docs/) and [AGENTS.md](AGENTS.md) first.

This crate and the Go version are kept identical. A change to how something renders goes into both, and `tools/parity` (a small Go program) regenerates the reference output the tests compare against. Run `cargo fmt`, `cargo clippy --all-targets` and `cargo test` before opening a PR.

## License

[The Unlicense](LICENSE) - public domain. The bundled `default_geometry.json` is Mojang's vanilla humanoid model, captured from a real client, included for interoperability. The pictures above are rendered by this crate from `testdata/bench-skin`.
