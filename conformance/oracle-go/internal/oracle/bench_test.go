package oracle

import (
	"os"
	"path/filepath"
	"testing"

	uddin "github.com/bonkzero404/uddin-lang"
	interp "github.com/bonkzero404/uddin-lang/interpreter"
)

// BenchmarkCoreScripts runs the scripts of benches/scripts the way the control plane does, one sandboxed
// execution per iteration. The Rust bench runs the same files, so the two columns are one workload.
func BenchmarkCoreScripts(b *testing.B) {
	prof, err := LoadProfile("../../../../profile/sandbox-profile.json")
	if err != nil {
		b.Fatal(err)
	}
	files, err := filepath.Glob("../../../../benches/scripts/*.din")
	if err != nil || len(files) == 0 {
		b.Fatalf("no bench scripts: %v", err)
	}
	vars := map[string]any{"method": "GET", "path": "/index.php", "query": "", "ip": "", "country": "", "ua": "curl/7.68", "body": "", "score": 3.5, "action": ""}
	for _, file := range files {
		src, err := os.ReadFile(file)
		if err != nil {
			b.Fatal(err)
		}
		prog, err := interp.ParseProgram(src)
		if err != nil {
			b.Fatal(err)
		}
		name := filepath.Base(file)
		b.Run(name[:len(name)-len(".din")], func(b *testing.B) {
			b.ReportAllocs()
			for i := 0; i < b.N; i++ {
				eng := uddin.NewSandboxed(prof.sandbox)
				eng.SetVariables(vars)
				if _, err := eng.ExecuteProgram(prog); err != nil {
					b.Fatal(err)
				}
			}
		})
	}
}
