# Changelog

## 0.2.2

- Fixed: poses now find bones case-insensitively both ways. The built-in
  motions and most example animations say `leftArm`, persona models name the
  bone `leftarm`, and persona skins stood still in walk, idle, wave, sneak and
  11 animations in all.

## 0.2.1

- `WireSkin` and `DecodedSkin`: a skin as a Bedrock packet carries it -
  raw RGBA, geometry, resource patch, animation list - decoded, the patch's
  model picked and persona faces attached; `.options()` renders it.
- `RenderOptions::write_png` and `write_gif`: render straight into any
  `std::io::Write`.
- Doc examples for `write_gif`, `example_animations` and `polygons`.
- `PolyMesh::polygons()` and `PolyVertex`: a poly mesh's polygons with each
  corner's position, normal and UV looked up; `GeometryValue::poly_mesh()`
  reads one from a geometry tree.

## 0.2.0

- Animations from bytes: `render_gif_bytes` and `render_frames_png`, with
  `AnimationBytesOptions` - encoded images in, GIF bytes or one PNG per frame
  out, the same as decoding first and rendering.
- `BytesOptions::animated` takes a persona skin's encoded animation images.
  This is a new public field, so a `BytesOptions { .. }` literal without
  `..Default::default()` needs it added.

## 0.1.0

The first release: a pixel-for-pixel port of bedrock-skin-go, including
persona skins drawn from their poly meshes, animation frames rasterized in
parallel (`AnimationOptions::workers`), and the invisible-skin detector.
