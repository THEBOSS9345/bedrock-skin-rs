package main

// Batch mode renders a skincheck plan (tools/skincheck writes it) with the
// Go library and records what every image looks like, so skincheck can judge
// the Go pictures with the same checks it applies to its own and say which of
// the two packages got a skin wrong:
//
//	go run . -batch ../skincheck/work [-threads 6]
//
// Each render records the hash of its pixels (the parity check) and the
// measurements the checks need: how much of the frame is opaque, how much of
// that is a colour from the skin, whether the model touches the edge, and the
// bounds of what was drawn. No images are kept.

import (
	"encoding/json"
	"fmt"
	"image"
	"image/draw"
	"os"
	"path/filepath"
	"runtime"
	"strings"
	"sync"
	"sync/atomic"
	"time"

	bedrockskin "github.com/THEBOSS9345/bedrock-skin-go"
)

type plan struct {
	Skins      []planSkin  `json:"skins"`
	Stills     []planStill `json:"stills"`
	Animations []planAnim  `json:"animations"`
}

type planSkin struct {
	ID         string `json:"id"`
	Identifier string `json:"identifier"`
	Geometry   bool   `json:"geometry"`
	Cape       bool   `json:"cape"`
}

type planStill struct {
	Name   string      `json:"name"`
	View   string      `json:"view"`
	Angle  string      `json:"angle"`
	Camera *planCamera `json:"camera"`
	Size   int         `json:"size"`
	Cape   bool        `json:"cape"`
}

type planAnim struct {
	Name  string `json:"name"`
	FPS   int    `json:"fps"`
	Size  int    `json:"size"`
	Angle string `json:"angle"`
}

type planCamera struct {
	Yaw, Pitch, FOV, Margin float64
}

// what one rendered image looks like, so the Rust side can run the same checks
// on it that it runs on its own render. Field names are short because there are
// millions of these in go.json.
type imgStat struct {
	Hash  string `json:"h,omitempty"`
	Error string `json:"e,omitempty"`
	Look  *look  `json:"l,omitempty"`
}

type look struct {
	Opaque   int  `json:"o"`
	Full     int  `json:"f"`
	Faithful int  `json:"c"`
	Edge     bool `json:"e"`
	X0       int  `json:"x0"`
	Y0       int  `json:"y0"`
	X1       int  `json:"x1"`
	Y1       int  `json:"y1"`
}

// ---- the progress bar ----

// Rewrites one stderr line as the skins go by, the same shape the Rust tool
// draws, redrawing at most ten times a second so the workers are not held up by
// the terminal.
const barWidth = 28

type progressBar struct {
	label    string
	total    int
	start    time.Time
	lastDraw time.Time
}

func newBar(label string, total int) *progressBar {
	b := &progressBar{label: label, total: total, start: time.Now(), lastDraw: time.Now().Add(-time.Second)}
	b.draw(0)
	return b
}

func (b *progressBar) tick(done int) {
	if time.Since(b.lastDraw) < 100*time.Millisecond && done < b.total {
		return
	}
	b.lastDraw = time.Now()
	b.draw(done)
}

func (b *progressBar) finish() {
	b.draw(b.total)
	fmt.Fprintln(os.Stderr)
}

func (b *progressBar) draw(done int) {
	frac := 1.0
	if b.total > 0 {
		frac = float64(done) / float64(b.total)
	}
	filled := int(frac*barWidth + 0.5)
	if filled > barWidth {
		filled = barWidth
	}
	secs := time.Since(b.start).Seconds()
	rate, eta := 0.0, 0.0
	if secs > 0 {
		rate = float64(done) / secs
		if rate > 0 {
			eta = float64(b.total-done) / rate
		}
	}
	fmt.Fprintf(os.Stderr, "\r%s [%s%s] %5.1f%%  %7d/%d  %6.0f/s  eta %5d:%02d   ",
		b.label, strings.Repeat("#", filled), strings.Repeat("-", barWidth-filled),
		frac*100, done, b.total, rate, int(eta)/60, int(eta)%60)
}

// ---- batch ----

func runBatch(dir string) {
	raw, err := os.ReadFile(filepath.Join(dir, "plan.json"))
	must(err)
	var p plan
	must(json.Unmarshal(raw, &p))
	ex := bedrockskin.ExampleAnimations()

	workers := *threads
	if workers <= 0 {
		workers = runtime.NumCPU()
	}
	if *limit > 0 && *limit < len(p.Skins) {
		p.Skins = p.Skins[:*limit]
	}

	results := make([]map[string][]imgStat, len(p.Skins))
	jobs := make(chan int)
	var wg sync.WaitGroup
	for w := 0; w < workers; w++ {
		wg.Add(1)
		go func() {
			defer wg.Done()
			for i := range jobs {
				results[i] = renderSkin(dir, p, p.Skins[i], ex)
			}
		}()
	}
	fmt.Fprintf(os.Stderr, "go: rendering %d skins on %d of %d cores\n", len(p.Skins), workers, runtime.NumCPU())
	bar := newBar("go", len(p.Skins))
	var sent int64
	for i := range p.Skins {
		jobs <- i
		bar.tick(int(atomic.AddInt64(&sent, 1)))
	}
	bar.finish()
	close(jobs)
	wg.Wait()

	out := make(map[string][]imgStat, len(results)*45)
	for i, r := range results {
		for k, v := range r {
			out[p.Skins[i].ID+"/"+k] = v
		}
	}
	b, err := json.Marshal(out)
	must(err)
	must(os.WriteFile(filepath.Join(dir, "go.json"), b, 0o644))
	fmt.Fprintf(os.Stderr, "go: wrote %d renders\n", len(out))
}

// renderSkin renders one skin's plan: each still is one entry, each animation
// one entry a frame. A render that fails records the error instead.
func renderSkin(dir string, p plan, s planSkin, ex map[string]*bedrockskin.Animation) map[string][]imgStat {
	out := map[string][]imgStat{}
	base := filepath.Join(dir, "skins", s.ID)
	tex := decodeFile(filepath.Join(base, "texture.png"))
	var geos []bedrockskin.Geometry
	if s.Geometry {
		raw, err := os.ReadFile(filepath.Join(base, "geometry.json"))
		must(err)
		if !bedrockskin.IsEmpty(raw) {
			if geos, err = bedrockskin.ParseGeometry(raw); err != nil {
				out["parse"] = []imgStat{{Error: err.Error()}}
				geos = nil
			}
		}
	}
	var cape image.Image
	if s.Cape {
		cape = decodeFile(filepath.Join(base, "cape.png"))
	}
	colours := skinColours(tex, cape)

	opts := func(view, angle string, size int) bedrockskin.Options {
		return bedrockskin.Options{
			Texture: tex, Geometry: geos, Identifier: s.Identifier,
			View: bedrockskin.View(view), Angle: bedrockskin.Angle(angle), Size: size,
		}
	}
	stat := func(img image.Image, err error) imgStat {
		if err != nil {
			return imgStat{Error: err.Error()}
		}
		return imgStat{Hash: hashImage(img), Look: lookOf(img, colours)}
	}
	for _, st := range p.Stills {
		if st.Cape && cape == nil {
			continue
		}
		o := opts(st.View, st.Angle, st.Size)
		if st.Cape {
			o.Cape = cape
		}
		if st.Camera != nil {
			o.Camera = &bedrockskin.Camera{Yaw: st.Camera.Yaw, Pitch: st.Camera.Pitch, FOV: st.Camera.FOV, Margin: st.Camera.Margin}
		}
		out[st.Name] = []imgStat{stat(o.Render())}
	}
	for _, a := range p.Animations {
		var anim bedrockskin.Animator
		if m, err := bedrockskin.ParseMotion(a.Name); err == nil {
			anim = m
		} else {
			anim = ex[a.Name]
		}
		frames, err := bedrockskin.RenderFrames(bedrockskin.AnimationOptions{Options: opts("", a.Angle, a.Size), Animation: anim, FPS: a.FPS})
		if err != nil {
			out["anim:"+a.Name] = []imgStat{{Error: err.Error()}}
			continue
		}
		ss := make([]imgStat, len(frames))
		for i, f := range frames {
			ss[i] = stat(f, nil)
		}
		out["anim:"+a.Name] = ss
	}
	return out
}

// skinColours is every colour the skin and cape are allowed to draw with: the
// pixels of at least half opacity. A render that is mostly other colours is
// drawing the wrong texture.
//
// The pixels are read as straight-alpha bytes, not through At().RGBA(), which
// would give premultiplied values and so a different colour for every pixel
// that is not fully opaque. skincheck builds its own set from the straight
// bytes, and the two sets have to match or the comparison is meaningless.
func skinColours(tex, cape image.Image) map[[3]uint8]bool {
	out := map[[3]uint8]bool{}
	add := func(img image.Image) {
		if img == nil {
			return
		}
		n := nrgba(img)
		for i := 0; i+3 < len(n.Pix); i += 4 {
			if n.Pix[i+3] < 128 {
				continue
			}
			out[[3]uint8{n.Pix[i], n.Pix[i+1], n.Pix[i+2]}] = true
		}
	}
	add(tex)
	add(cape)
	return out
}

// lookOf measures a rendered image: how much is drawn, how much of it is opaque,
// how much of that is a colour from the skin, whether it reaches the edge, and
// the bounds of what was drawn.
func lookOf(img image.Image, colours map[[3]uint8]bool) *look {
	n := nrgba(img)
	b := n.Bounds()
	w, h := b.Dx(), b.Dy()
	l := &look{X0: w, Y0: h, X1: 0, Y1: 0}
	for y := 0; y < h; y++ {
		for x := 0; x < w; x++ {
			i := n.PixOffset(b.Min.X+x, b.Min.Y+y)
			r, g, bb, a := n.Pix[i], n.Pix[i+1], n.Pix[i+2], n.Pix[i+3]
			if a == 0 {
				continue
			}
			l.Opaque++
			if x == 0 || y == 0 || x == w-1 || y == h-1 {
				l.Edge = true
			}
			if x < l.X0 {
				l.X0 = x
			}
			if y < l.Y0 {
				l.Y0 = y
			}
			if x > l.X1 {
				l.X1 = x
			}
			if y > l.Y1 {
				l.Y1 = y
			}
			if a == 255 {
				l.Full++
				if colours[[3]uint8{r, g, bb}] {
					l.Faithful++
				}
			}
		}
	}
	return l
}

func decodeFile(path string) image.Image {
	b, err := os.ReadFile(path)
	must(err)
	img, err := bedrockskin.DecodeImage(b)
	must(err)
	return img
}

// nrgba gives the image as straight-alpha NRGBA with no padding, so the pixel
// walk and the hash see the same bytes the Rust side hashes.
func nrgba(img image.Image) *image.NRGBA {
	b := img.Bounds()
	n, ok := img.(*image.NRGBA)
	if !ok || n.Stride != b.Dx()*4 || b.Min != (image.Point{}) {
		n = image.NewNRGBA(image.Rect(0, 0, b.Dx(), b.Dy()))
		draw.Draw(n, n.Bounds(), img, b.Min, draw.Src)
	}
	return n
}

// hashImage is FNV-1a 64 over the width and height (little-endian uint32) and
// then the straight-alpha RGBA bytes, as skincheck computes it.
func hashImage(img image.Image) string {
	b := img.Bounds()
	n := nrgba(img)
	h := uint64(14695981039346656037)
	add := func(c byte) {
		h ^= uint64(c)
		h *= 1099511628211
	}
	for _, v := range []int{b.Dx(), b.Dy()} {
		for i := 0; i < 4; i++ {
			add(byte(v >> (8 * i)))
		}
	}
	for _, c := range n.Pix {
		add(c)
	}
	return fmt.Sprintf("%016x", h)
}

// isBatch reports whether the arguments ask for batch mode.
func isBatch(dir string) bool { return strings.TrimSpace(dir) != "" }
