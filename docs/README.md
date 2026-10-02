# bedrock-skin documentation

Everything about how this library turns a Minecraft Bedrock skin into an image, why it is built the way it is, and how to build on top of it. The API itself is documented on [docs.rs](https://docs.rs/bedrock-skin).

| I want to... | Read |
| --- | --- |
| Find the function for something | [api-reference.md](api-reference.md) |
| Solve a specific problem | [recipes.md](recipes.md) |
| Check a skin for invisible parts | [api-reference.md](api-reference.md#invisibility-detection) |
| Understand what a Bedrock client actually sends | [skin-data.md](skin-data.md) |
| Understand geometry.json | [geometry-format.md](geometry-format.md) |
| Pick values out of a geometry file by path | [geometry-format.md](geometry-format.md#picking-values-out-of-a-file) |
| Understand how a mesh becomes pixels | [rendering-pipeline.md](rendering-pipeline.md) |
| Understand framing, bone scoping, cameras | [views-and-cameras.md](views-and-cameras.md) |
| Animate a skin | [animation.md](animation.md) |
| Know why a Minecraft animation does nothing on a model | [animation.md](animation.md#which-minecraft-animations-work) |
| Know *why* something is done a particular way | [design-decisions.md](design-decisions.md) |
| Know how this crate stays identical to the Go version | [design-decisions.md](design-decisions.md#the-rust-version) |

## The short version

A Bedrock skin is two things: a **texture** (a flat PNG atlas) and a **model** (`geometry.json`, a tree of named bones holding boxes). Rendering means walking that bone tree, turning every box into triangles with texture coordinates, pointing a camera at the result, and rasterizing it on the CPU.

The library has two halves:

- **Rendering** - texture and geometry in, image out.
- **Invisibility detection** - the same inputs, asking "is this skin invisible, or partly invisible?" and answering with a report of which body parts are missing.

Most of these pages are shared with [bedrock-skin-go](https://github.com/THEBOSS9345/bedrock-skin-go), because the two versions work the same way.
