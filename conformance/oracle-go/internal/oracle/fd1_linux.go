package oracle

import (
	"os"
	"syscall"
)

// The fork's print() with no argument writes its newline to file descriptor 1 with a raw system call, even in a
// sandbox that has its own stdout, so that line would land in the result stream and be missing from the captured
// print output. CaptureFd1 moves the real stdout to the returned file and points descriptor 1 at a pipe that
// runs read back, in order, into the captured output. Linux only.

const fSetPipeSz = 1031 // F_SETPIPE_SZ

var fd1Pipe = -1 // read end of the pipe behind descriptor 1; -1 when not captured

// CaptureFd1 redirects descriptor 1 as described above. Both ends of the pipe are non-blocking, so a run that
// writes more than the 1 MiB the pipe holds between two reads loses the excess instead of hanging. The returned
// function undoes the redirection.
func CaptureFd1() (results *os.File, restore func(), err error) {
	orig, err := syscall.Dup(1)
	if err != nil {
		return nil, nil, err
	}
	var p [2]int
	if err := syscall.Pipe2(p[:], syscall.O_NONBLOCK|syscall.O_CLOEXEC); err != nil {
		syscall.Close(orig)
		return nil, nil, err
	}
	// A smaller pipe than asked for is not an error.
	_, _, _ = syscall.Syscall(syscall.SYS_FCNTL, uintptr(p[1]), fSetPipeSz, 1<<20)
	if err := syscall.Dup3(p[1], 1, 0); err != nil {
		syscall.Close(orig)
		syscall.Close(p[0])
		syscall.Close(p[1])
		return nil, nil, err
	}
	syscall.Close(p[1])
	fd1Pipe = p[0]
	return os.NewFile(uintptr(orig), "stdout"), func() {
		_ = syscall.Dup3(orig, 1, 0)
		syscall.Close(p[0])
		fd1Pipe = -1
	}, nil
}

// drainFd1 appends what was written to descriptor 1 since the last call to c.
func drainFd1(c *capture) {
	if fd1Pipe < 0 {
		return
	}
	buf := make([]byte, 32<<10)
	for {
		n, err := syscall.Read(fd1Pipe, buf)
		if n <= 0 || err != nil {
			return
		}
		c.append(buf[:n])
	}
}
