# API reference

Everything the crate exports, grouped by job. Each item's full documentation is on [docs.rs](https://docs.rs/bedrock-skin); this page is the map. The right-hand column is the same thing in [bedrock-skin-go](https://github.com/THEBOSS9345/bedrock-skin-go), for anyone moving between the two.

```rust
use bedrock_skin::*;
```

## Rendering

| Rust | What it does | Go |
| --- | --- | --- |
| `render(&RenderOptions)` | Renders a square image. | `Render` |
| `RenderOptions::new(&texture)` | Options with everything defaulted: full body, front, 512px, default model. Builder methods `.geometry()`, `.identifier()`, `.cape()`, `.view()`, `.angle()`, `.parts()`, `.camera()`, `.size()`, `.pose()` set one field each, and `.animated(kind, &texture)` adds a persona animation image; the fields are public too. | `Options` |
| `AnimatedTexture`, `AnimatedType` | A persona animation image and its kind (`Face`, `Body32`, `Body128`; `from_protocol` maps the protocol's number). Each draws its `geometry.animated_*` entry. | `AnimatedTexture`, `AnimatedType` |
| `RenderOptions::render()`, `render_png()` | Renders these options, to an image or to PNG bytes. | `Options.Render`, `RenderPNG` |
| `render_bytes(&BytesOptions)` | Encoded texture, geometry, cape and persona animation (`animated`) bytes in, PNG bytes out. | `RenderBytes` |
| `WireSkin { .. }.decode()` | A skin as a packet carries it - raw RGBA images, geometry, resource patch, animations - decoded into a `DecodedSkin`; `.options()` renders it, `WireSkin::skin()` checks it for invisibility. | `WireSkin.Options`, `WireSkin.Skin` |
| `RenderOptions::write_png(w)`, `write_gif(w, &opts)` | Render straight into any `std::io::Write`: the same bytes as `render_png` and `render_gif`. | `Options.WritePNG`, `WriteGIF` |
| `render_gif_bytes(&AnimationBytesOptions)`, `render_frames_png(..)` | The same for animations: bytes in, GIF bytes or one PNG per frame out. `AnimationBytesOptions::new(bytes, &animator)` with `.fps()`, `.frames()`, `.workers()`. | `RenderGIFBytes`, `RenderFramesPNG` |
| `render_2d(&texture, view, size)` | The flat paper-doll crop for geometry that draws nothing. | `Render2D` |
| `View` | `Body`, `Chest`, `Head`, `Avatar`. | `ViewBody`... |
| `Angle` | `Front`, `Iso`. | `AngleFront`, `AngleIso` |
| `Camera` | Explicit `yaw`, `pitch`, `fov`, `margin`; zero fields take defaults. | `Camera` |
| `DEFAULT_SIZE` | 512. | `DefaultSize` |
| `parse_view`, `parse_angle`, `parse_parts` | Read request parameters, rejecting unknown names. | `ParseView`, `ParseAngle`, `ParseParts` |

## Equipment

See [equipment.md](equipment.md).

| Rust | What it does | Go |
| --- | --- | --- |
| `Armor { helmet, chestplate, leggings, boots, elytra }`, `Armor::set(&layer1, &layer2)` | Armor and elytra worn over the skin, one texture per piece; `.armor()` on `RenderOptions`. The elytra takes the chestplate's slot. | `Armor`, `ArmorSet` |
| `Held { item, flat, adjust }`, `Held::new(&item)` | An item in one hand; `.right_hand()`, `.left_hand()`. Upright as a tool unless `flat`. | `Held` |
| `ItemAdjust { offset, rotation, scale }` | Moves, turns or resizes a held item from the game's placement, about its grip. | `ItemAdjust` |
| `Scale { model, parts }` | The figure's size in the image, and per-bone scales; `.scale()`. | `Scale` |
| `RenderOptions::equipment()`, `.hide_skin(true)` | The equipment alone, without the skin; no texture needed. | `Options.HideSkin` |
| `render_item(&ItemOptions)`, `ItemOptions::new(&item)` | An item on its own, extruded as held; `.angle()`, `.camera()`, `.size()`, `.adjust()`. | `RenderItem` |
| `render_item_frames`, `render_item_gif(&ItemAnimationOptions)` | The item spinning, one turn a loop (`duration`, `fps`, `frames`). | `RenderItemFrames`, `RenderItemGIF` |
| `ArmorBytes`, `HeldBytes`, `BytesOptions::{armor, right_hand, left_hand, scale, hide_skin}` | The same from encoded bytes. | `ArmorBytes`, `HeldBytes` |
| `render_item_bytes(&ItemBytesOptions)`, `render_item_gif_bytes(&ItemAnimationBytesOptions)` | Items on their own from encoded bytes. | `RenderItemBytes`, `RenderItemGIFBytes` |

## Images

| Rust | What it does | Go |
| --- | --- | --- |
| `decode_image(&bytes)` | PNG or JPEG to an `RgbaImage`. No size limit: check dimensions first for untrusted input. | `DecodeImage` |
| `image_dimensions(&bytes)` | Width and height from the header alone. | `ImageDimensions` |
| `encode_png(&img)` | An image to PNG bytes. | `EncodePNG` |
| `texture_from_rgba(pixels, w, h)` | Raw RGBA as Bedrock sends it, checked against its dimensions. | `TextureFromRGBA` |

Textures and results are `image::RgbaImage`: straight (not premultiplied) 8-bit RGBA.

## Geometry

| Rust | What it does | Go |
| --- | --- | --- |
| `parse_geometry(&bytes)` | Both formats into `Vec<Geometry>`; `null` gives none. | `ParseGeometry` |
| `is_empty(&bytes)` | Nothing, or the literal `null` a client sends for a built-in model. | `IsEmpty` |
| `default_geometry()` | The vanilla wide, slim and cape models. | `DefaultGeometry` |
| `select_geometry(&geos, id)` | The named entry, else the one with the most cubes. | `SelectGeometry` |
| `find_cape(&geos)` | The entry with a `cape` bone. | `FindCape` |
| `complexity(&geos)` | Total bones and cubes, to bound untrusted uploads. | `Complexity` |
| `parse_resource_patch(&bytes)` | The patch's `default` and `cape` identifiers. | `ParseResourcePatch` |
| `Geometry`, `Bone`, `Cube`, `Locator`, `FaceUv` | The model, as the file has it. `Geometry::bone_by_name`, `children`, `locator`, `total_cubes`, `has_mesh`; `Bone::mesh`; `Cube::box_uv`, `face_uvs`. | same names |
| `PolyMesh`, `PolyVertex` | A bone's poly mesh: `normalized_uvs`, `positions`, `normals`, `uvs`, `polys`; `polygons()` gives every polygon with each corner's `position`, `normal` and `uv` looked up. `GeometryValue::poly_mesh()` reads one from a geometry tree. | `PolyMesh`, `PolyVertex` |
| `parse_geometry_tree(&bytes)` | The whole file, every field, for picking values by path: `GeometryTree::select`, `get`, `identifiers`, `geometries`. | `ParseGeometryTree` |
| `GeometryValue` | A picked value: `path`, `value`, and `as_f64`, `as_f64s`, `as_str`, `bone`, `cube`, `locator`, `decode`, `json`. | `GeometryValue` |

## Animation

| Rust | What it does | Go |
| --- | --- | --- |
| `Motion` | `Walk`, `Idle`, `Wave`, `Sneak`; `Motion::ALL`, `parse_motion`. | `MotionWalk`... |
| `Animator` | The trait both motions and animations implement: `duration()`, `pose(t)`. | `Animator` |
| `parse_animations(&bytes)` | A Bedrock animation file, by name. | `ParseAnimations` |
| `Animation` | One animation: `name`, `looping`, `hold_on_last_frame`, `length`, `bones()`, `missing_bones(&geometry)`. | `Animation` |
| `example_animations()` | The 33 bundled examples. | `ExampleAnimations` |
| `Pose`, `BonePose` | How bones move from rest; set `RenderOptions::pose` for a still. | `Pose`, `BonePose` |
| `AnimationOptions::new(options, &animator)` | What to animate; `.fps()`, `.frames()`, `.workers()` (frames rasterized at once; 0 means every core, 1 one at a time - the images are the same either way). | `AnimationOptions` |
| `render_frames`, `render_gif` | Every frame with one shared camera, or a looping GIF. | `RenderFrames`, `RenderGIF` |
| `prepare_frames(&AnimationOptions)`, `Frames` | Frames prepared once and drawn one at a time at a moving camera: `Frames::len`, `Frames::draw(i, size, Option<Camera>)`. | `PrepareFrames`, `Frames` |

## Invisibility detection

| Rust | What it does | Go |
| --- | --- | --- |
| `Skin::new(texture, geometry)` | A skin to question; the analysis runs once, on first use. | `NewSkin` |
| `Skin::with_options(texture, geometry, SkinOptions)` | The same, with your thresholds. | `NewSkinWithOptions` |
| `Skin::report()` | The `SkinReport`. Also `ok()`, `is_invisible()`, `is_suspicious()`, `invisible_parts()`, `parts()`. | `Skin.Report`... |
| `SkinReport` | `verdict`, `visible_parts`, `total_parts`, `parts`; serde, in the Go version's JSON shape. `ok()` is false for a default report. | `SkinReport` |
| `Verdict` | `Unknown`, `Ok`, `Suspicious`, `Invisible`. | `VerdictUnknown`... |
| `PartReport`, `PartVisibility` | One part: `Visible`, `Invisible`, `Suspicious` (some opaque pixels, too few) or `Tiny` (geometry too small). | same names |
| `validate_skin_invisibility`, `validate_skin_visibility`, `validate_geometry_size`, `is_skin_invisible`, `is_skin_tiny` | The lower-level checks the report is built from. | `ValidateSkinInvisibility`... |
| `DEFAULT_MIN_VISIBLE_FRACTION`, `DEFAULT_MIN_GEOMETRY_SIZE`, `DEFAULT_MIN_VISIBLE_PARTS`, `DEFAULT_MIN_VISIBLE_ALPHA` | The default thresholds. | `DefaultMin...` |

How the verdict is reached:

- **With geometry**, every bone with cubes is checked where its UVs actually point in the texture, and a bone too small to see counts as invisible however opaque it is. 0-1 standard parts visible is `Invisible`, 2-3 is `Suspicious`.
- **Without geometry**, the standard layout is assumed, so the verdict is lenient: only no visible parts is `Invisible`.
- **Persona skins** have their poly meshes measured; parts drawn only by an animated entry (the head), whose texture the detector is not given, are trusted. Bones that draw nothing are trusted visible. Geometry that does not parse is checked like no geometry, so garbage cannot switch the detector off.
- Overlay layers (hat, jacket, sleeves, pants) count toward the part they cover; a cape never counts.

## Errors

Every fallible function returns `Result<_, Error>`. All variants but `Encode` are bad input, so a service can answer them with a 4xx: `NoTexture`, `NoGeometry`, `NoMatchingParts`, `EmptyView`, `UnknownView`, `UnknownAngle`, `UnknownMotion`, `NoAnimations`, `NoGeometryModels`, `Json`, `Geometry`, `Animation`, `ResourcePatch`, `Image`, `Pixels`.
