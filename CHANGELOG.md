# Changelog

## 0.3.0

- Armor: `RenderOptions::armor` wears a helmet, chestplate, leggings and boots, one
  texture per piece as a resource pack lays them out, on vanilla's armor
  model. `Armor::set` wears a full set; `BytesOptions::armor`
  takes them encoded (`ArmorBytes`).
- Elytra: `Armor::elytra`, on vanilla's elytra model in its resting pose,
  in the chestplate's slot.
- Held items: `RenderOptions::right_hand` and `left_hand` each hold an item sprite
  (`Held`), extruded one texel deep and placed where the game places it,
  from the model's `rightItem` or `leftItem`, with the arm held forward as
  vanilla's holding animation holds it. Tools and weapons are held upright;
  `Held::flat` holds anything else flat. `Held::adjust` (`ItemAdjust`) moves,
  turns or resizes an item the game's placement does not suit.
  `BytesOptions` takes them encoded (`HeldBytes`).
- `RenderOptions::scale`: the figure's size in the image, and per-bone scales that
  carry armor and items with them.
- `RenderOptions::hide_skin` (`RenderOptions::equipment()`) draws the equipment alone, so a helmet, the elytra or a
  held item renders by itself; no texture is then needed. `render_item` and
  `render_item_bytes` render an item sprite on its own, and `render_item_gif`,
  `render_item_frames` and `render_item_gif_bytes` spin it.
- Equipment moves with every pose and animation, and head and avatar views
  show only the helmet.
- Fixed: one-pixel slivers along the edges of thin, edge-on faces, and a
  gap down the diagonal of a face larger than the image, both inherited
  from fauxgl's rasterizer; pixels off the image's side wrapping into the
  next row; and every cube's bottom face mapped the wrong way round. Every
  render can differ by a few edge pixels from 0.2.2.

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
