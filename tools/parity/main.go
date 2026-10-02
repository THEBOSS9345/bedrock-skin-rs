// Command parity renders reference output with bedrock-skin-go, the
// library this crate ports, into testdata/parity. The Rust tests in
// tests/parity.rs check the port against it: the same images, poses,
// reports and query results.
//
// Run it from the repository root after changing either library:
//
//	cd tools/parity && go run . -out ../../testdata/parity
package main

import (
	"encoding/json"
	"flag"
	"fmt"
	"image"
	"image/color"
	"math"
	"os"
	"path/filepath"
	"sort"

	bedrockskin "github.com/THEBOSS9345/bedrock-skin-go"
)

var out = flag.String("out", "../../testdata/parity", "where to write the fixtures")

func main() {
	flag.Parse()
	must(os.MkdirAll(filepath.Join(*out, "renders"), 0o755))
	must(os.MkdirAll(filepath.Join(*out, "frames"), 0o755))

	renders()
	frames()
	writeJSON("poses.json", poses())
	writeJSON("reports.json", reports())
	writeJSON("queries.json", queries())
	writeJSON("geometry.json", geometries())
	writeJSON("trig.json", trig())
}

// ---- inputs ----

func testTexture() *image.NRGBA {
	img := image.NewNRGBA(image.Rect(0, 0, 64, 64))
	for y := 0; y < 64; y++ {
		for x := 0; x < 64; x++ {
			img.Set(x, y, color.NRGBA{R: uint8(x * 4), G: uint8(y * 4), B: 128, A: 255})
		}
	}
	return img
}

// semiTexture has every alpha from 0 to 255, to exercise the alpha test,
// blending, and the 2D fallback's premultiplying.
func semiTexture() *image.NRGBA {
	img := image.NewNRGBA(image.Rect(0, 0, 64, 64))
	for y := 0; y < 64; y++ {
		for x := 0; x < 64; x++ {
			img.Set(x, y, color.NRGBA{R: uint8(x*4 + 3), G: uint8(255 - y*4), B: uint8(x * y), A: uint8((x*7 + y*13) % 256)})
		}
	}
	return img
}

// customTexture is 128 square, transparent down its right quarter.
func customTexture() *image.NRGBA {
	img := image.NewNRGBA(image.Rect(0, 0, 128, 128))
	for y := 0; y < 128; y++ {
		for x := 0; x < 128; x++ {
			a := uint8(255)
			if x >= 96 {
				a = 0
			}
			img.Set(x, y, color.NRGBA{R: uint8(x * 2), G: uint8(y * 2), B: uint8(x ^ y), A: a})
		}
	}
	return img
}

func legacyTexture() *image.NRGBA {
	img := image.NewNRGBA(image.Rect(0, 0, 64, 32))
	for y := 0; y < 32; y++ {
		for x := 0; x < 64; x++ {
			img.Set(x, y, color.NRGBA{R: uint8(x * 4), G: uint8(y * 8), B: 60, A: 255})
		}
	}
	return img
}

// headOnly is the test texture with only the head's front face opaque.
func headOnly() *image.NRGBA {
	img := testTexture()
	for y := 0; y < 64; y++ {
		for x := 0; x < 64; x++ {
			if !(x >= 8 && x < 16 && y >= 8 && y < 16) {
				img.Pix[img.PixOffset(x, y)+3] = 0
			}
		}
	}
	return img
}

func read(name string) []byte {
	b, err := os.ReadFile(filepath.Join(*out, name))
	must(err)
	return b
}

func benchSkin() (image.Image, []bedrockskin.Geometry) {
	tex, err := bedrockskin.DecodeImage(readRoot("testdata/bench-skin/texture.png"))
	must(err)
	geos, err := bedrockskin.ParseGeometry(readRoot("testdata/bench-skin/geometry.json"))
	must(err)
	return tex, geos
}

func readRoot(rel string) []byte {
	b, err := os.ReadFile(filepath.Join(*out, "..", "..", rel))
	must(err)
	return b
}

func parse(name string) []bedrockskin.Geometry {
	geos, err := bedrockskin.ParseGeometry(read(name))
	must(err)
	return geos
}

// ---- renders ----

func renders() {
	bench, benchGeo := benchSkin()
	test, semi, custom := testTexture(), semiTexture(), customTexture()
	customGeo, personaGeo, legacyGeo := parse("custom-geometry.json"), parse("persona-geometry.json"), parse("legacy-geometry.json")

	scaled := bedrockskin.Pose{
		"head":     {Scale: [3]float64{1.5, 1.5, 1.5}, Scaled: true, Rotation: [3]float64{0, 30, 0}},
		"rightarm": {Scale: [3]float64{0, 0, 0}, Scaled: true},
		"leftLeg":  {Position: [3]float64{0, 2, -3}, Rotation: [3]float64{-40, 0, 0}},
	}

	cases := map[string]bedrockskin.Options{
		"bench-body-iso":     {Texture: bench, Geometry: benchGeo, Angle: bedrockskin.AngleIso, Size: 128},
		"bench-avatar":       {Texture: bench, Geometry: benchGeo, View: bedrockskin.ViewAvatar, Size: 100},
		"bench-cape":         {Texture: bench, Geometry: benchGeo, Cape: test, Size: 120},
		"bench-chest-camera": {Texture: bench, Geometry: benchGeo, View: bedrockskin.ViewChest, Camera: &bedrockskin.Camera{Yaw: -40, Pitch: 10}, Size: 90},
		"custom-body":        {Texture: custom, Geometry: customGeo, Angle: bedrockskin.AngleIso, Size: 128},
		"custom-back":        {Texture: custom, Geometry: customGeo, Camera: &bedrockskin.Camera{Yaw: 160, Pitch: 30}, Cape: test, Size: 100},
		"custom-head":        {Texture: custom, Geometry: customGeo, View: bedrockskin.ViewHead, Size: 80},
		"custom-parts":       {Texture: custom, Geometry: customGeo, Parts: []string{"tail", "horn"}, Size: 64},
		"semi-body":          {Texture: semi, Angle: bedrockskin.AngleIso, Size: 96},
		"close-camera":       {Texture: test, Camera: &bedrockskin.Camera{Yaw: 30, Pitch: 20, FOV: 70, Margin: 0.35}, Size: 96},
		"inside-camera":      {Texture: test, Camera: &bedrockskin.Camera{Yaw: 180, Pitch: -5, FOV: 90, Margin: 0.1}, Size: 96},
		"persona-body":       {Texture: semi, Geometry: personaGeo, Size: 100},
		"persona-chest":      {Texture: semi, Geometry: personaGeo, View: bedrockskin.ViewChest, Size: 77},
		"persona-head":       {Texture: semi, Geometry: personaGeo, View: bedrockskin.ViewHead, Size: 64},
		"persona-avatar-8":   {Texture: semi, Geometry: personaGeo, View: bedrockskin.ViewAvatar, Size: 8},
		"persona-128":        {Texture: custom, Geometry: personaGeo, Size: 50},
		"legacy-body":        {Texture: test, Geometry: legacyGeo, Size: 64},
		"legacy-alpha":       {Texture: legacyTexture(), Geometry: legacyGeo, Identifier: "geometry.alpha", Size: 64},
		"sneak-still":        {Texture: test, Pose: bedrockskin.MotionSneak.Pose(0.4), Size: 96},
		"scaled-pose":        {Texture: test, Pose: scaled, Angle: bedrockskin.AngleIso, Size: 96},
		"tiny":               {Texture: test, View: bedrockskin.ViewAvatar, Size: 3},
	}
	for name, opts := range cases {
		b, err := opts.RenderPNG()
		must(err)
		must(os.WriteFile(filepath.Join(*out, "renders", name+".png"), b, 0o644))
	}
}

// ---- animation frames ----

func frames() {
	test := testTexture()
	customGeo := parse("custom-geometry.json")
	ex := bedrockskin.ExampleAnimations()
	mol, err := bedrockskin.ParseAnimations(read("molang-test.animation.json"))
	must(err)

	anims := map[string]bedrockskin.Animator{
		"walk": bedrockskin.MotionWalk, "idle": bedrockskin.MotionIdle,
		"wave": bedrockskin.MotionWave, "sneak": bedrockskin.MotionSneak,
	}
	for _, n := range []string{"dance", "backflip", "jumping_jacks", "spin", "sword_swing", "zombie_walk", "levitate", "sit", "swim", "airplane"} {
		anims[n] = ex["animation.player."+n]
	}
	for name, a := range anims {
		writeFrames(name, bedrockskin.AnimationOptions{Options: bedrockskin.Options{Texture: test, Size: 64}, Animation: a, FPS: 6})
	}
	writeFrames("molang", bedrockskin.AnimationOptions{
		Options:   bedrockskin.Options{Texture: customTexture(), Geometry: customGeo, Angle: bedrockskin.AngleIso, Size: 72},
		Animation: mol["animation.parity.molang"], FPS: 5,
	})
}

func writeFrames(name string, opts bedrockskin.AnimationOptions) {
	imgs, err := bedrockskin.RenderFrames(opts)
	must(err)
	for i, img := range imgs {
		b, err := bedrockskin.EncodePNG(img)
		must(err)
		must(os.WriteFile(filepath.Join(*out, "frames", fmt.Sprintf("%s-%02d.png", name, i)), b, 0o644))
	}
}

// ---- poses ----

type poseCase struct {
	Animation string
	T         float64
	Pose      bedrockskin.Pose
}

func poses() []poseCase {
	anims := map[string]bedrockskin.Animator{}
	for n, a := range bedrockskin.ExampleAnimations() {
		anims[n] = a
	}
	mol, err := bedrockskin.ParseAnimations(read("molang-test.animation.json"))
	must(err)
	for n, a := range mol {
		anims[n] = a
	}
	for _, m := range bedrockskin.Motions() {
		anims[string(m)] = m
	}
	names := make([]string, 0, len(anims))
	for n := range anims {
		names = append(names, n)
	}
	sort.Strings(names)
	var out []poseCase
	for _, n := range names {
		for _, t := range []float64{0, 0.13, 0.5, 0.77, 1.3, 2.9, 7.25} {
			out = append(out, poseCase{n, t, anims[n].Pose(t)})
		}
	}
	return out
}

// ---- reports ----

type reportCase struct {
	Report        bedrockskin.SkinReport
	Visibility    bedrockskin.SkinVisibilityResult
	GeometrySize  bedrockskin.GeometrySizeResult
	IsInvisible   bool
	IsTiny        bool
	InvisibleList []string
}

func reports() map[string]reportCase {
	transparent := image.NewNRGBA(image.Rect(0, 0, 64, 64))
	custom := read("custom-geometry.json")
	cases := map[string]struct {
		tex  image.Image
		geo  []byte
		opts bedrockskin.SkinOptions
	}{
		"standard":    {testTexture(), nil, bedrockskin.SkinOptions{}},
		"transparent": {transparent, nil, bedrockskin.SkinOptions{}},
		"head-only":   {headOnly(), nil, bedrockskin.SkinOptions{}},
		"legacy32":    {legacyTexture(), nil, bedrockskin.SkinOptions{}},
		"custom":      {customTexture(), custom, bedrockskin.SkinOptions{}},
		"custom-128":  {customTexture(), read("legacy-geometry.json"), bedrockskin.SkinOptions{}},
		"persona":     {semiTexture(), read("persona-geometry.json"), bedrockskin.SkinOptions{}},
		"garbage-geo": {testTexture(), []byte("{nope"), bedrockskin.SkinOptions{}},
		"semi":        {semiTexture(), nil, bedrockskin.SkinOptions{}},
		"strict":      {semiTexture(), nil, bedrockskin.SkinOptions{MinVisibleFraction: 0.9, MinVisibleParts: 6}},
		"big-min":     {customTexture(), custom, bedrockskin.SkinOptions{MinGeometrySize: 3}},
		"bench":       {decode(readRoot("testdata/bench-skin/texture.png")), readRoot("testdata/bench-skin/geometry.json"), bedrockskin.SkinOptions{}},
	}
	out := map[string]reportCase{}
	for name, c := range cases {
		s := bedrockskin.NewSkinWithOptions(c.tex, c.geo, c.opts)
		out[name] = reportCase{
			Report:        s.Report(),
			Visibility:    bedrockskin.ValidateSkinVisibility(c.tex, c.geo, 0.3),
			GeometrySize:  bedrockskin.ValidateGeometrySize(c.geo, 0),
			IsInvisible:   bedrockskin.IsSkinInvisible(c.tex),
			IsTiny:        bedrockskin.IsSkinTiny(c.geo),
			InvisibleList: s.InvisibleParts(),
		}
	}
	return out
}

func decode(b []byte) image.Image {
	img, err := bedrockskin.DecodeImage(b)
	must(err)
	return img
}

// ---- geometry ----

type queryCase struct {
	File, Path string
	Values     []bedrockskin.GeometryValue
}

func queries() []queryCase {
	paths := []string{
		"", "*", "geometry.parity.custom/bones/rightArm/pivot", "*/bones/*/cubes/*/size", "*/bones/-1/name",
		"*/description", "*/bones/HEAD/rotation", "geometry.zeta", "geometry.zeta/bones/0/cubes/0/uv",
		"*/bones/head/locators/*", "*/bones/horn/cubes/0/uv/*/uv_size", "nope/bones", "*/bones/99", "/ */description/texture_width /",
		"*/BONES/body/cubes/-2/inflate",
	}
	var out []queryCase
	for _, f := range []string{"custom-geometry.json", "legacy-geometry.json", "persona-geometry.json"} {
		tree, err := bedrockskin.ParseGeometryTree(read(f))
		must(err)
		for _, p := range paths {
			vs := tree.Select(p)
			if vs == nil {
				vs = []bedrockskin.GeometryValue{}
			}
			out = append(out, queryCase{f, p, vs})
		}
	}
	return out
}

type geometryCase struct {
	Input      string
	Error      bool
	Geometries []bedrockskin.Geometry
	Patch      *bedrockskin.ResourcePatch
}

func geometries() []geometryCase {
	inputs := []string{
		string(read("custom-geometry.json")),
		string(read("legacy-geometry.json")),
		string(read("persona-geometry.json")),
		string(readRoot("testdata/bench-skin/geometry.json")),
		`null`, `[]`, `"x"`, `{`, `{}`,
		`{"minecraft:geometry":[{"description":{"identifier":"a","texture_width":"64"},"bones":[{"name":"b"}]}]}`,
		`{"Minecraft:Geometry":[{"Description":{"Identifier":"case"},"Bones":[{"NAME":"b","Pivot":[1,2,3],"pivot":[4,5,6],"cubes":[{"origin":[0,0,0],"size":[1,1,1],"uv":[0,0],"Inflate":null}]}]}]}`,
		`{"minecraft:geometry":[{"description":{"identifier":"n"},"bones":[{"name":"b","pivot":null,"rotation":[1,null,3],"mirror":null,"locators":{"a":[1,2],"b":{"offset":"x","rotation":[1,2,3]},"c":null}}]}]}`,
		`{"format_version":"1.8.0","geometry.x":{"bones":[{"name":"b","mirror":1}]},"geometry.y":{"texturewidth":32,"bones":[{"name":"c"}]}}`,
		`{"minecraft:geometry":[]}`,
		`{"minecraft:geometry":[{"bones":[]}],"geometry.legacy":{"bones":[{"name":"l"}]}}`,
		`{"minecraft:geometry":{"description":{}}}`,
		`{"minecraft:geometry":[5]}`,
	}
	var out []geometryCase
	for _, in := range inputs {
		geos, err := bedrockskin.ParseGeometry([]byte(in))
		c := geometryCase{Input: in, Error: err != nil, Geometries: geos}
		out = append(out, c)
	}
	for _, in := range []string{``, `null`, `{"geometry":{"default":"geometry.humanoid.customSlim"}}`, `{"geometry":{"default":"a","cape":"b"}}`, `{"GEOMETRY":{"Default":"x"}}`, `{"geometry":{"default":5}}`, `{`} {
		p, err := bedrockskin.ParseResourcePatch([]byte(in))
		c := geometryCase{Input: "patch:" + in, Error: err != nil}
		if err == nil {
			c.Patch = &p
		}
		out = append(out, c)
	}
	return out
}

// ---- trigonometry ----

type trigCase struct {
	X             float64
	Sin, Cos, Tan string
}

func trig() []trigCase {
	var out []trigCase
	xs := []float64{0, 1, -1, 0.5, 2, 3, math.Pi, math.Pi / 2, 0.6108652381980153, 0.4363323129985824, 1e-9, 1e5, 123456.789, 5e8, 1e10, 1e20, 1e300, -7.25, 0.30543261909900765}
	for i := 0; i < 400; i++ {
		xs = append(xs, float64(i)*0.0371-7.3)
	}
	for _, x := range xs {
		out = append(out, trigCase{x, bits(math.Sin(x)), bits(math.Cos(x)), bits(math.Tan(x))})
	}
	return out
}

func bits(f float64) string { return fmt.Sprintf("%016x", math.Float64bits(f)) }

// ---- helpers ----

func writeJSON(name string, v any) {
	b, err := json.MarshalIndent(v, "", " ")
	must(err)
	must(os.WriteFile(filepath.Join(*out, name), append(b, '\n'), 0o644))
}

func must(err error) {
	if err != nil {
		panic(err)
	}
}
