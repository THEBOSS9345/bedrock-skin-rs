# Changelog

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
