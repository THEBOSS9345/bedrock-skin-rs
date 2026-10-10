// Command parity renders reference output with bedrock-skin-go, the
// library this crate ports, into testdata/parity. The Rust tests in
// tests/parity.rs check the port against it: the same images, poses,
// reports and query results.
//
// Run it after changing either library:
//
//	cd tools/parity && go run .

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

var (
	out = flag.String("out", "../../testdata/parity", "where to write the fixtures")
)

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

// armorTexture is an armor layer, 64x32 as the game's are, with a
// transparent patch where real chestplates leave the lower arm bare.
func armorTexture(tint uint8) *image.NRGBA {
	img := image.NewNRGBA(image.Rect(0, 0, 64, 32))
	for y := 0; y < 32; y++ {
		for x := 0; x < 64; x++ {
			a := uint8(255)
			if x >= 40 && x < 56 && y >= 26 {
				a = 0
			}
			img.Set(x, y, color.NRGBA{R: tint, G: uint8(120 + x*2), B: uint8(140 + y*3), A: a})
		}
	}
	return img
}

// itemTexture is an item sprite of side n: a diagonal blade whose alpha
// sweeps every value, to pin the held item's alpha cut-off.
func itemTexture(n int) *image.NRGBA {
	img := image.NewNRGBA(image.Rect(0, 0, n, n))
	for y := 0; y < n; y++ {
		for x := 0; x < n; x++ {
			d := x + y - (n - 1)
			if d < -1 || d > 1 {
				continue
			}
			img.Set(x, y, color.NRGBA{R: uint8(x * 16), G: uint8(200 - y*5), B: uint8(60 + d*50), A: uint8((x*37 + y*11) % 256)})
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

// faceTexture stands in for a persona face animation: 32x64, two frames,
// the top frame's hat half partly transparent.
func faceTexture() *image.NRGBA {
	img := image.NewNRGBA(image.Rect(0, 0, 32, 64))
	for y := 0; y < 64; y++ {
		for x := 0; x < 32; x++ {
			a := uint8(255)
			if y >= 16 && y < 32 {
				a = uint8((x * y * 5) % 256)
			}
			img.Set(x, y, color.NRGBA{R: uint8(x * 8), G: uint8(y * 4), B: 200, A: a})
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
	meshGeo, oddGeo, companionGeo := parse("persona-mesh-geometry.json"), parse("persona-mesh-odd.json"), parse("persona-companion-geometry.json")
	face := []bedrockskin.AnimatedTexture{{Type: bedrockskin.AnimatedFace, Texture: faceTexture()}}
	body128 := []bedrockskin.AnimatedTexture{{Type: bedrockskin.AnimatedBody128, Texture: semi}, {Type: bedrockskin.AnimatedFace, Texture: faceTexture()}}

	scaled := bedrockskin.Pose{
		"head":     {Scale: [3]float64{1.5, 1.5, 1.5}, Scaled: true, Rotation: [3]float64{0, 30, 0}},
		"rightarm": {Scale: [3]float64{0, 0, 0}, Scaled: true},
		"leftLeg":  {Position: [3]float64{0, 2, -3}, Rotation: [3]float64{-40, 0, 0}},
	}

	diamond := bedrockskin.ArmorSet(armorTexture(40), armorTexture(150))
	winged := diamond
	winged.Elytra = armorTexture(220)
	sword := bedrockskin.Held{Item: itemTexture(16)}
	flat := bedrockskin.Held{Item: itemTexture(16), Flat: true}

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
		"cape-sneak":         {Texture: test, Cape: test, Pose: bedrockskin.MotionSneak.Pose(0.4), Camera: &bedrockskin.Camera{Yaw: 150, Pitch: 15}, Size: 96},
		"cape-swim":          {Texture: test, Cape: test, Armor: bedrockskin.Armor{Chestplate: armorTexture(40)}, Pose: bedrockskin.ExampleAnimations()["animation.player.swim"].Pose(0.5), Camera: &bedrockskin.Camera{Yaw: 200, Pitch: 30}, Size: 96},
		"scaled-pose":        {Texture: test, Pose: scaled, Angle: bedrockskin.AngleIso, Size: 96},
		"tiny":               {Texture: test, View: bedrockskin.ViewAvatar, Size: 3},
		"mesh-body":          {Texture: test, Geometry: meshGeo, Size: 96},
		"mesh-face-iso":      {Texture: test, Geometry: meshGeo, Animated: face, Angle: bedrockskin.AngleIso, Size: 96},
		"mesh-face-head":     {Texture: test, Geometry: meshGeo, Animated: face, View: bedrockskin.ViewHead, Size: 80},
		"mesh-face-avatar":   {Texture: semi, Geometry: meshGeo, Animated: face, View: bedrockskin.ViewAvatar, Size: 64},
		"mesh-face-chest":    {Texture: test, Geometry: meshGeo, Animated: face, View: bedrockskin.ViewChest, Cape: semi, Size: 72},
		"mesh-face-back":     {Texture: test, Geometry: meshGeo, Animated: face, Cape: semi, Camera: &bedrockskin.Camera{Yaw: 150, Pitch: 20}, Size: 96},
		"mesh-parts-hat":     {Texture: test, Geometry: meshGeo, Animated: face, Parts: []string{"HAT", "leftArm"}, Size: 64},
		"mesh-odd":           {Texture: test, Geometry: oddGeo, Angle: bedrockskin.AngleIso, Size: 96},
		"mesh-odd-head":      {Texture: semi, Geometry: oddGeo, View: bedrockskin.ViewHead, Size: 64},
		"mesh-odd-chest":     {Texture: test, Geometry: oddGeo, View: bedrockskin.ViewChest, Size: 64},
		"mesh-companion":     {Texture: test, Geometry: companionGeo, Animated: body128, Angle: bedrockskin.AngleIso, Size: 96},
		"armor-iso":          {Texture: test, Armor: diamond, Angle: bedrockskin.AngleIso, Size: 96},
		"armor-back":         {Texture: semi, Armor: diamond, Cape: test, Camera: &bedrockskin.Camera{Yaw: 160, Pitch: 25}, Size: 96},
		"armor-mixed-slim":   {Texture: test, Identifier: "geometry.humanoid.customSlim", Armor: bedrockskin.Armor{Helmet: armorTexture(200), Boots: armorTexture(90)}, Angle: bedrockskin.AngleIso, Size: 80},
		"armor-avatar":       {Texture: test, Armor: diamond, RightHand: sword, View: bedrockskin.ViewAvatar, Size: 64},
		"armor-chest":        {Texture: test, Armor: diamond, RightHand: sword, LeftHand: flat, View: bedrockskin.ViewChest, Size: 72},
		"elytra-back":        {Texture: test, Armor: winged, Camera: &bedrockskin.Camera{Yaw: 170, Pitch: 15}, Size: 96},
		"elytra-side":        {Texture: semi, Armor: winged, RightHand: sword, Camera: &bedrockskin.Camera{Yaw: -70, Pitch: -20}, Size: 96},
		"held-side":          {Texture: test, RightHand: sword, Camera: &bedrockskin.Camera{Yaw: -70, Pitch: 10}, Size: 96},
		"held-24":            {Texture: semi, RightHand: bedrockskin.Held{Item: itemTexture(24)}, Angle: bedrockskin.AngleIso, Size: 96},
		"held-left":          {Texture: test, LeftHand: sword, Camera: &bedrockskin.Camera{Yaw: 60, Pitch: 10}, Size: 96},
		"held-both-flat":     {Texture: test, RightHand: flat, LeftHand: bedrockskin.Held{Item: itemTexture(24), Flat: true}, Angle: bedrockskin.AngleIso, Size: 96},
		"held-adjust":        {Texture: test, RightHand: bedrockskin.Held{Item: itemTexture(16), Adjust: bedrockskin.ItemAdjust{Offset: [3]float64{0.5, 2, -1.25}, Rotation: [3]float64{37, -20, 11}, Scale: 1.3}}, LeftHand: bedrockskin.Held{Item: itemTexture(16), Flat: true, Adjust: bedrockskin.ItemAdjust{Rotation: [3]float64{0, 90, 0}, Scale: 0.7}}, Camera: &bedrockskin.Camera{Yaw: -35, Pitch: 15}, Size: 96},
		"held-slim":          {Texture: test, Identifier: "geometry.humanoid.customSlim", RightHand: sword, LeftHand: sword, Pose: bedrockskin.MotionWave.Pose(0.3), Size: 80},
		"held-parts":         {Texture: test, RightHand: sword, LeftHand: sword, Armor: diamond, Parts: []string{"rightArm"}, Size: 64},
		"held-custom":        {Texture: custom, Geometry: customGeo, Armor: winged, RightHand: sword, LeftHand: sword, Angle: bedrockskin.AngleIso, Size: 96},
		"held-mesh":          {Texture: test, Geometry: meshGeo, Animated: face, Armor: diamond, RightHand: sword, LeftHand: flat, Angle: bedrockskin.AngleIso, Size: 96},
		"scale-parts":        {Texture: test, Armor: diamond, RightHand: sword, Scale: bedrockskin.Scale{Parts: map[string]float64{"HEAD": 1.6, "rightarm": 1.3, "leftLeg": 0, "Body": 0.9}}, Pose: scaled, Angle: bedrockskin.AngleIso, Size: 96},
		"scale-model-big":    {Texture: test, Armor: winged, RightHand: sword, Scale: bedrockskin.Scale{Model: 1.7}, Angle: bedrockskin.AngleIso, Size: 80},
		"scale-model-small":  {Texture: semi, Scale: bedrockskin.Scale{Model: 0.45}, Camera: &bedrockskin.Camera{Yaw: 20, Pitch: 5, Margin: 1.1}, Size: 80},
		"solo-armor":         {HideSkin: true, Armor: winged, RightHand: sword, Angle: bedrockskin.AngleIso, Size: 96},
		"solo-helmet":        {HideSkin: true, Armor: diamond, View: bedrockskin.ViewHead, Size: 64},
		"solo-hand":          {HideSkin: true, LeftHand: flat, Camera: &bedrockskin.Camera{Yaw: 50, Pitch: 10}, Size: 64},
		"solo-cape-parts":    {HideSkin: true, Cape: test, Armor: diamond, Parts: []string{"cape", "leftLeg"}, Size: 64},
	}
	for name, opts := range cases {
		b, err := opts.RenderPNG()
		must(err)
		must(os.WriteFile(filepath.Join(*out, "renders", name+".png"), b, 0o644))
	}

	items := map[string]bedrockskin.ItemOptions{
		"item-front":  {Item: itemTexture(16), Size: 64},
		"item-iso":    {Item: itemTexture(24), Angle: bedrockskin.AngleIso, Size: 80},
		"item-camera": {Item: itemTexture(16), Camera: &bedrockskin.Camera{Yaw: 130, Pitch: -25, FOV: 50, Margin: 1.4}, Size: 72},
		"item-adjust": {Item: itemTexture(16), Adjust: bedrockskin.ItemAdjust{Offset: [3]float64{3, -1, 2}, Rotation: [3]float64{20, 33, -45}, Scale: 2.5}, Size: 64},
	}
	for name, opts := range items {
		img, err := bedrockskin.RenderItem(opts)
		must(err)
		b, err := bedrockskin.EncodePNG(img)
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
	writeFrames("mesh-walk", bedrockskin.AnimationOptions{
		Options:   bedrockskin.Options{Texture: test, Geometry: parse("persona-mesh-geometry.json"), Animated: []bedrockskin.AnimatedTexture{{Type: bedrockskin.AnimatedFace, Texture: faceTexture()}}, Angle: bedrockskin.AngleIso, Size: 64},
		Animation: bedrockskin.MotionWalk, FPS: 4,
	})
	winged := bedrockskin.ArmorSet(armorTexture(40), armorTexture(150))
	winged.Elytra = armorTexture(220)
	equipped := bedrockskin.Options{Texture: test, Armor: winged, RightHand: bedrockskin.Held{Item: itemTexture(16)}, LeftHand: bedrockskin.Held{Item: itemTexture(16), Flat: true, Adjust: bedrockskin.ItemAdjust{Rotation: [3]float64{10, 0, 0}}}, Angle: bedrockskin.AngleIso, Size: 64}
	spin, err := bedrockskin.RenderItemFrames(bedrockskin.ItemAnimationOptions{ItemOptions: bedrockskin.ItemOptions{Item: itemTexture(16), Camera: &bedrockskin.Camera{Pitch: 15}, Adjust: bedrockskin.ItemAdjust{Rotation: [3]float64{0, 0, 20}}, Size: 48}, Duration: 1.5, FPS: 4})
	must(err)
	for i, f := range spin {
		b, err := bedrockskin.EncodePNG(f)
		must(err)
		must(os.WriteFile(filepath.Join(*out, "frames", fmt.Sprintf("item-spin-%02d.png", i)), b, 0o644))
	}
	solo := bedrockskin.Options{HideSkin: true, Armor: winged, RightHand: bedrockskin.Held{Item: itemTexture(16)}, Angle: bedrockskin.AngleIso, Size: 64}
	writeFrames("solo-walk", bedrockskin.AnimationOptions{Options: solo, Animation: bedrockskin.MotionWalk, FPS: 4})
	writeFrames("armored-walk", bedrockskin.AnimationOptions{Options: equipped, Animation: bedrockskin.MotionWalk, FPS: 4})
	writeFrames("armored-sneak", bedrockskin.AnimationOptions{Options: equipped, Animation: bedrockskin.MotionSneak, FPS: 4})
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
		"mesh":        {testTexture(), read("persona-mesh-geometry.json"), bedrockskin.SkinOptions{}},
		"mesh-semi":   {semiTexture(), read("persona-mesh-geometry.json"), bedrockskin.SkinOptions{}},
		"mesh-odd":    {semiTexture(), read("persona-mesh-odd.json"), bedrockskin.SkinOptions{}},
		"mesh-128":    {customTexture(), read("persona-mesh-geometry.json"), bedrockskin.SkinOptions{}},
		"companion":   {headOnly(), read("persona-companion-geometry.json"), bedrockskin.SkinOptions{}},
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
		string(read("persona-mesh-odd.json")),
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
