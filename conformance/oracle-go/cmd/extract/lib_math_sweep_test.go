package main

import (
	"bufio"
	"encoding/binary"
	"fmt"
	"math"
	"os"
	"strconv"
	"strings"
	"testing"
)

// TestMathSweep writes what the Go math library gives for generated arguments, for the Rust tests of the float
// builtins to read: a header line, then for each function a line "name arity count" and count records of arity
// arguments and the result, each a little-endian 64-bit pattern. It does nothing unless UDDIN_SWEEP_OUT names
// the file. UDDIN_SWEEP_N is the number of arguments for each function (default 50000).
//
//	UDDIN_SWEEP_OUT=/tmp/sweep.bin go test ./cmd/extract -run TestMathSweep
func TestMathSweep(t *testing.T) {
	path := os.Getenv("UDDIN_SWEEP_OUT")
	if path == "" {
		t.Skip("UDDIN_SWEEP_OUT is not set")
	}
	n := 50000
	if s := os.Getenv("UDDIN_SWEEP_N"); s != "" {
		v, err := strconv.Atoi(s)
		if err != nil || v < 1 {
			t.Fatalf("UDDIN_SWEEP_N: %q", s)
		}
		n = v
	}
	f, err := os.Create(path)
	if err != nil {
		t.Fatal(err)
	}
	defer f.Close()
	w := bufio.NewWriterSize(f, 1<<20)
	defer w.Flush()
	fmt.Fprintf(w, "uddin-sweep 1 cpu-flags=%s\n", cpuFlags())

	put := func(vs ...float64) {
		var b [8]byte
		for _, v := range vs {
			binary.LittleEndian.PutUint64(b[:], math.Float64bits(v))
			w.Write(b[:])
		}
	}
	unary := []struct {
		name string
		f    func(float64) float64
		kind int
	}{
		{"sin", math.Sin, 0}, {"cos", math.Cos, 0}, {"tan", math.Tan, 0},
		{"asin", math.Asin, 1}, {"acos", math.Acos, 1}, {"atan", math.Atan, 0},
		{"sinh", math.Sinh, 0}, {"cosh", math.Cosh, 0}, {"tanh", math.Tanh, 0},
		{"cbrt", math.Cbrt, 0}, {"exp", math.Exp, 0}, {"exp2", math.Exp2, 0},
		{"log", math.Log, 0}, {"log10", math.Log10, 0}, {"log2", math.Log2, 0},
		{"sqrt", math.Sqrt, 0}, {"floor", math.Floor, 0}, {"ceil", math.Ceil, 0}, {"trunc", math.Trunc, 0}, {"round", math.Round, 0},
	}
	for i, u := range unary {
		xs := sweepInputs(uint64(i)+1, n, u.kind)
		fmt.Fprintf(w, "%s 1 %d\n", u.name, len(xs))
		for _, x := range xs {
			put(x, u.f(x))
		}
	}
	binary2 := []struct {
		name string
		f    func(a, b float64) float64
	}{
		{"atan2", math.Atan2}, {"pow", math.Pow}, {"logb", func(a, b float64) float64 { return math.Log(a) / math.Log(b) }},
	}
	for i, b := range binary2 {
		ps := sweepPairs(uint64(100+i), n, b.name)
		fmt.Fprintf(w, "%s 2 %d\n", b.name, len(ps))
		for _, p := range ps {
			put(p[0], p[1], b.f(p[0], p[1]))
		}
	}
}

// cpuFlags names the CPU features that choose a path in the library: the exp routine of amd64 uses fused
// multiply-add when the CPU has it.
func cpuFlags() string {
	data, err := os.ReadFile("/proc/cpuinfo")
	if err != nil {
		return "unknown"
	}
	for _, line := range strings.Split(string(data), "\n") {
		if strings.HasPrefix(line, "flags") {
			var have []string
			for _, flag := range strings.Fields(line) {
				switch flag {
				case "fma", "avx", "avx2", "sse4_1":
					have = append(have, flag)
				}
			}
			return strings.Join(have, ",")
		}
	}
	return "unknown"
}

// splitmix is a small generator that the sweep owns, so that its arguments do not depend on the library.
type splitmix uint64

func (s *splitmix) next() uint64 {
	*s += 0x9E3779B97F4A7C15
	z := uint64(*s)
	z = (z ^ (z >> 30)) * 0xBF58476D1CE4E5B9
	z = (z ^ (z >> 27)) * 0x94D049BB133111EB
	return z ^ (z >> 31)
}

// unit is a number in [0, 1).
func (s *splitmix) unit() float64 { return float64(s.next()>>11) / (1 << 53) }

// sweepInputs are n arguments: the ones that tend to break a port first, then random ones. kind 1 is for the
// functions of a domain of [-1, 1].
func sweepInputs(seed uint64, n, kind int) []float64 {
	var xs []float64
	add := func(vs ...float64) { xs = append(xs, vs...) }
	around := func(v float64, ulps int) {
		for _, d := range []float64{math.Inf(-1), math.Inf(1)} {
			x := v
			for i := 0; i < ulps; i++ {
				x = math.Nextafter(x, d)
				add(x)
			}
		}
	}
	add(0, math.Copysign(0, -1), 1, -1, 0.5, -0.5, 2, -2, 3, -3, 10, -10, 0.1, -0.1, 1.5, -1.5, 100, 1e6, -1e6, math.Inf(1), math.Inf(-1), math.NaN(),
		math.Float64frombits(0xFFF8000000000001), math.MaxFloat64, -math.MaxFloat64, 0x1p-1022, -0x1p-1022, 5e-324, -5e-324, math.Pi, -math.Pi, math.E)
	// Every power of two, with the numbers next to it, and the subnormals.
	for e := -1074; e <= 1023; e++ {
		p := math.Ldexp(1, e)
		add(p, math.Nextafter(p, 0), math.Nextafter(p, math.Inf(1)))
		if e%5 == 0 {
			add(-p)
		}
	}
	for k := uint64(1); k <= 40; k++ {
		add(math.Float64frombits(k), math.Float64frombits(1<<52-k), math.Float64frombits(1<<52+k))
	}
	// Multiples of a quarter turn, the points where the circular functions change octant, and the neighbours.
	for k := -300; k <= 300; k++ {
		v := float64(k) * (math.Pi / 4)
		add(v, math.Nextafter(v, 0), math.Nextafter(v, math.Inf(1)))
	}
	// Numbers close to one, and around the arguments where the algorithms switch branches.
	around(1, 40)
	around(-1, 40)
	for _, v := range []float64{0.5, 0.625, 0.66, 0.7, 2.414213562373095, 21, 22, 44.01484596555653, 88.02969193111305, 709.782712893384, -745.1332191019411, 1.0 / (1 << 28), 1e-14, 1e-7,
		0.7071067811865476, 1.4142135623730951, 0.5 * math.Pi, math.Pi, 1 << 29, 1e9, 1e10, 1e15, 1e22, 1e100, 1e300, 1 << 52, 1 << 53, 1 << 62, 1 << 63, 1 << 64} {
		add(v, -v)
		around(v, 6)
		around(-v, 3)
	}
	// The fraction that the assembly of the logarithm treats differently, in every binade.
	for e := -1074; e <= 1023; e += 1 {
		v := math.Ldexp(0.7071067811865476, e)
		add(v, math.Nextafter(v, 0), math.Nextafter(v, math.Inf(1)))
	}
	// Exponents where the range reduction of exp steps: halves of ln 2.
	for k := -1100; k <= 1100; k += 1 {
		v := (float64(k) + 0.5) / math.Log2E
		add(v, math.Nextafter(v, 0), math.Nextafter(v, math.Inf(1)))
	}
	// Integers and halves.
	for i := -300; i <= 300; i++ {
		add(float64(i), float64(i)+0.5, float64(i)/7)
	}
	rng := splitmix(seed * 0x1234567)
	for len(xs) < n {
		var x float64
		switch rng.next() % 4 {
		case 0:
			x = math.Float64frombits(rng.next())
		case 1:
			x = math.Pow(10, rng.unit()*40-20) * float64(1-2*int(rng.next()&1))
		case 2:
			x = rng.unit()*20 - 10
			if kind == 1 {
				x = rng.unit()*2.4 - 1.2
			}
		default:
			x = math.Ldexp(rng.unit()+1, int(rng.next()%200)-100) * float64(1-2*int(rng.next()&1))
		}
		xs = append(xs, x)
	}
	return xs[:n]
}

// sweepPairs are n pairs of arguments of a function of two.
func sweepPairs(seed uint64, n int, name string) [][2]float64 {
	var ps [][2]float64
	special := []float64{0, math.Copysign(0, -1), 1, -1, 0.5, -0.5, 2, -2, 3, -3, 10, -10, 0.1, 1.5, -1.5, 7, 0.7071067811865476, 2.414213562373095, 1e-300, -1e-300, 5e-324, -5e-324, 0x1p-1022,
		1e300, -1e300, math.MaxFloat64, -math.MaxFloat64, math.Inf(1), math.Inf(-1), math.NaN(), math.Pi, -math.Pi, math.E, 1 << 53, -(1 << 53), 1 << 62, 1 << 63, -(1 << 63), 1 << 64, 1e19, 0.3, 100, -100, 1024, -1024, 1075, -1075, 0.66, 4.5, -4.5}
	for _, a := range special {
		for _, b := range special {
			ps = append(ps, [2]float64{a, b})
		}
	}
	rng := splitmix(seed * 0x7654321)
	logUniform := func(lo, hi float64) float64 { return math.Pow(10, lo+rng.unit()*(hi-lo)) }
	sign := func() float64 { return float64(1 - 2*int(rng.next()&1)) }
	switch name {
	case "pow":
		for _, base := range []float64{10, 2, math.E, 0.1, 0.5, 1.5, -2, -10, 3, 7, 0.9} {
			for e := -420; e <= 420; e++ {
				ps = append(ps, [2]float64{base, float64(e)})
			}
			for e := -40; e <= 40; e++ {
				ps = append(ps, [2]float64{base, float64(e) + 0.5}, [2]float64{base, float64(e) / 3})
			}
		}
	case "atan2":
		for i := 0; i < 4000; i++ {
			// A ratio near a threshold of the arctangent, in every quadrant.
			for _, r := range []float64{0.66, 2.414213562373095, 1, 0.7071067811865476, 1e-8, 1e8} {
				x := logUniform(-3, 3) * sign()
				ps = append(ps, [2]float64{x * r * (1 + (rng.unit()-0.5)*1e-9), x})
			}
		}
	}
	for len(ps) < n {
		var p [2]float64
		switch rng.next() % 5 {
		case 0:
			p = [2]float64{math.Float64frombits(rng.next()), math.Float64frombits(rng.next())}
		case 1:
			p = [2]float64{logUniform(-8, 8) * sign(), logUniform(-8, 8) * sign()}
		case 2:
			p = [2]float64{rng.unit()*20 - 10, rng.unit()*20 - 10}
		case 3:
			p = [2]float64{logUniform(-3, 3), float64(int(rng.next()%80) - 40)}
		default:
			p = [2]float64{logUniform(-3, 3), (rng.unit() - 0.5) * 40}
		}
		if name == "logb" {
			p = [2]float64{math.Abs(p[0]), math.Abs(p[1])}
		}
		ps = append(ps, p)
	}
	return ps[:n]
}
