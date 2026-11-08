package main

/*
#include <stdlib.h>
#include <string.h>
*/
import "C"

import (
	"os"
	"runtime/debug"
	"runtime/metrics"
	"sync"
	"unsafe"

	"github.com/xtls/xray-core/core"
	"github.com/xtls/xray-core/features/stats"
	_ "github.com/xtls/xray-core/main/distro/all"
)

// The extension is killed by jetsam at 50MB of physical footprint, and Go's
// default GOGC lets the heap double before collecting. Swift, NetworkExtension
// and hev-socks5-tunnel need the rest of the budget, so Go gets a soft cap and
// collects earlier.
const (
	goMemoryLimitBytes = 30 * 1024 * 1024
	goGCPercent        = 50
)

var (
	mu       sync.Mutex
	instance *core.Instance
)

func init() {
	debug.SetMemoryLimit(goMemoryLimitBytes)
	debug.SetGCPercent(goGCPercent)
}

func setErr(buf *C.char, n C.int, msg string) {
	if buf == nil || n <= 1 {
		return
	}
	max := int(n) - 1
	bytes := []byte(msg)
	if len(bytes) > max {
		bytes = bytes[:max]
	}
	slot := unsafe.Slice((*byte)(unsafe.Pointer(buf)), max+1)
	copy(slot, bytes)
	slot[len(bytes)] = 0
}

//export XrayStart
func XrayStart(configPath *C.char, assetDir *C.char, errBuf *C.char, errLen C.int) C.int {
	mu.Lock()
	defer mu.Unlock()
	releaseInstance()
	if assetDir != nil {
		_ = os.Setenv("XRAY_LOCATION_ASSET", C.GoString(assetDir))
	}
	data, err := os.ReadFile(C.GoString(configPath))
	if err != nil {
		setErr(errBuf, errLen, err.Error())
		return -1
	}
	next, err := core.StartInstance("json", data)
	if err != nil {
		setErr(errBuf, errLen, err.Error())
		return -1
	}
	instance = next
	return 0
}

//export XrayStop
func XrayStop() {
	mu.Lock()
	defer mu.Unlock()
	releaseInstance()
}

// releaseInstance closes the running core and hands its memory back to the OS.
// The extension process can survive a quick disconnect and reconnect, and the
// iOS jetsam limit is too tight to carry the previous session's geodata heap.
// Callers must hold mu.
func releaseInstance() {
	if instance == nil {
		return
	}
	_ = instance.Close()
	instance = nil
	debug.FreeOSMemory()
}

//export XrayRunning
func XrayRunning() C.int {
	mu.Lock()
	defer mu.Unlock()
	if instance == nil {
		return 0
	}
	return 1
}

func counter(name string) uint64 {
	if instance == nil {
		return 0
	}
	raw := instance.GetFeature(stats.ManagerType())
	mgr, ok := raw.(stats.Manager)
	if !ok || mgr == nil {
		return 0
	}
	item := mgr.GetCounter(name)
	if item == nil || item.Value() < 0 {
		return 0
	}
	return uint64(item.Value())
}

//export XrayProxyUplink
func XrayProxyUplink() C.ulonglong {
	mu.Lock()
	defer mu.Unlock()
	return C.ulonglong(counter("outbound>>>proxy>>>traffic>>>uplink"))
}

//export XrayProxyDownlink
func XrayProxyDownlink() C.ulonglong {
	mu.Lock()
	defer mu.Unlock()
	return C.ulonglong(counter("outbound>>>proxy>>>traffic>>>downlink"))
}

// XrayGoMemory is the memory the Go runtime holds from the OS, so the
// extension can tell Go growth apart from Swift, NetworkExtension and hev.
//
//export XrayGoMemory
func XrayGoMemory() C.ulonglong {
	samples := []metrics.Sample{
		{Name: "/memory/classes/total:bytes"},
		{Name: "/memory/classes/heap/released:bytes"},
	}
	metrics.Read(samples)
	total := samples[0].Value.Uint64()
	released := samples[1].Value.Uint64()
	if released > total {
		return 0
	}
	return C.ulonglong(total - released)
}

// XrayGCCycles counts completed GC cycles since the extension started.
//
//export XrayGCCycles
func XrayGCCycles() C.ulonglong {
	samples := []metrics.Sample{{Name: "/gc/cycles/total:gc-cycles"}}
	metrics.Read(samples)
	return C.ulonglong(samples[0].Value.Uint64())
}

// XrayGCCPUMillis is the CPU time the collector has used since start. The
// runtime estimates it, so treat it as a share of total CPU, not an exact cost.
//
//export XrayGCCPUMillis
func XrayGCCPUMillis() C.ulonglong {
	samples := []metrics.Sample{{Name: "/cpu/classes/gc/total:cpu-seconds"}}
	metrics.Read(samples)
	return C.ulonglong(samples[0].Value.Float64() * 1000)
}

func main() {}
