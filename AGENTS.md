# Notes for coding agents

Contributions written with AI assistance are welcome. Read this first.

## What this project is

A pure Rust library that renders Minecraft Bedrock skins to images and detects invisible skins. It is a port of [bedrock-skin-go](https://github.com/THEBOSS9345/bedrock-skin-go), and **the two must produce identical output**: same pixels, same pose values to the bit, same reports. `tests/parity.rs` and `tests/golden.rs` enforce it against reference output the Go library wrote.

## Read before changing code

| File | When it matters |
| --- | --- |
| [docs/design-decisions.md](docs/design-decisions.md) | **Before changing anything that looks wrong** - especially "The Rust version" at the end |
| [docs/rendering-pipeline.md](docs/rendering-pipeline.md) | `mesh.rs`, `render.rs`, `raster.rs` |
| [docs/geometry-format.md](docs/geometry-format.md) | `geometry.rs`, `geoquery.rs` |
| [docs/skin-data.md](docs/skin-data.md) | How skins arrive over the wire |
| [docs/animation.md](docs/animation.md) | `animation.rs`, `animfile.rs`, `molang.rs` |

## Rules that are easy to break

- **Do not "simplify" arithmetic.** `raster.rs`, `mesh.rs` and `render.rs` do floating-point operations in the same order as the Go code. Reordering, fusing (`mul_add`), or swapping in `f64::min`/`max`, `sin`, `cos` or `tan` changes the last bit and breaks parity. Use `gomath`.
- **Geometry is read by `jsonread.rs`, not serde derive**, to match Go's decoder exactly (case-insensitive keys, null handling, type errors that do not stop reading).
- **A behaviour change goes into both versions.** Change the Go library, regenerate the fixtures (`cd tools/parity && go run .`), then change the Rust until `cargo test` passes. Never edit `testdata/parity` or `testdata/golden` by hand.
- Comments stay brief and point into `docs/`.

## Before a PR

`cargo fmt`, `cargo clippy --all-targets -- -D warnings`, `cargo test`.

## Layout

```
src/lib.rs          exports, Error, image helpers, render_bytes
src/render.rs       RenderOptions, views, framing, the camera
src/mesh.rs         bones and cubes to triangles
src/polymesh.rs     poly meshes: persona skins
src/raster.rs       the rasterizer: fauxgl's, ported and specialised
src/render2d.rs     the flat fallback, for geometry that draws nothing
src/geometry.rs     geometry.json, both formats
src/geoquery.rs     GeometryTree: values by path
src/jsonread.rs     reading JSON with Go's rules
src/animation.rs    Pose, Motion, frames and GIFs
src/animfile.rs     Bedrock animation files
src/molang.rs       Molang expressions
src/gomath.rs       Go's trigonometry, ported
src/invisible.rs    the visibility checks
src/detect.rs       Skin and SkinReport
tools/parity        the Go program that writes testdata/parity
tools/skincheck     renders a skin database with both libraries and rates it
```

## Checking against real skins

`tools/skincheck` renders every skin in a scout database with both
libraries - every view, the cape, every frame of every animation - compares
them byte for byte, and rates each skin with checks for blank, cut-off,
off-centre, wrong-coloured and motionless renders and for the detector
disagreeing with the picture:

```
cd tools/skincheck
cargo run --release -- export --scout <folder with scout.db and skins/>
cargo run --release -- go    [--threads N] [--looks]
cargo run --release -- check [--threads N]
```

It writes `work/report.html`. `work/` holds real players' skins: it is
git-ignored and must stay local, as must any database. `--threads` sets
how many cores it uses (half by default); a full run of 36,000 skins takes
about a quarter of an hour on 14. The Go side records only hashes, since
matching hashes mean identical images; `go --looks` also measures every Go
image, for looking into a skin whose images differ. It is the measuring instrument, so do not tune its
checks to make a library change look better.
