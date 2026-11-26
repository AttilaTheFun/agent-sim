#!/bin/bash
# Boots Apple's iOS simulator runtime natively on arm64 Linux, under Darling built with patches/darling-arm64 (no
# translator: the runtime's arm64 code runs as it is). x86Sim provides the device (cli/libexec/mksimdevice.py), the
# programs that stand in for CoreSimulator and the Mac inside the simulator (host/*/arm64, prebuilt) and simhost,
# which plays CoreSimulator's part of the launchd handshake (built for arm64 by scripts/build-simhost-arm64.sh).
#
#   scripts/boot-arm64.sh [seconds]      (default: until stopped)
#
# Env: X86SIM_ROOT (an x86Sim checkout with the runtime fetched; default ~/x86sim), A64_DEVICE (default
# $X86SIM_ROOT/var/device-a64), A64_PHONE (<n>: another phone, device-a64-<n> with its own Darling prefix
# ~/.darling-<n>), A64_JOBS (default: boot-a64.sh's slim list, MIN + UI_SLIM), DPREFIX (Darling prefix, default
# ~/.darling), X86SIM_METAL (default all: Metal for every process through x86Sim's metalhostd and mtlvk, built for
# arm64 by scripts/build-mtlvk-arm64.sh; 0 = none, and backboardd then draws nothing). Frames go to simfb's ring
# (SIMFB_RING in sim.env; x86Sim's cli/libexec/fb2png.py makes a PNG of the newest).
set -e
X=${X86SIM_ROOT:-$HOME/x86sim}
export X86SIM_ROOT=$X X86SIM_PREFIX=${X86SIM_PREFIX:-/usr/local} X86SIM_METAL=${X86SIM_METAL-all}
# A64_PHONE=<n>: another phone beside the first, for several tests at once (agent-sim xctest takes whichever phone is
# free): device-a64-<n>, with its own Darling prefix and state (Metal and audio sockets)
if [ -n "${A64_PHONE:-}" ]; then
	A64_DEVICE=${A64_DEVICE:-$X/var/device-a64-$A64_PHONE}
	export DPREFIX=${DPREFIX:-$HOME/.darling-$A64_PHONE}
	export X86SIM_STATE=${X86SIM_STATE:-$X/var/run-$A64_PHONE}
	mkdir -p "$X86SIM_STATE"
fi
D=${A64_DEVICE:-$X/var/device-a64}
export X86SIM_DEVICE=$D
DPREFIX=${DPREFIX:-$HOME/.darling}
HERE=$(cd "$(dirname "$0")" && pwd)
SIMHOST=$X/host/simhost/simhost-arm64
[ -x "$SIMHOST" ] || "$HERE/build-simhost-arm64.sh" "$X/host/simhost/simhost.c" "$SIMHOST"

# boot-a64.sh's job list for A64_JOBS=std (its default; slim: MIN + UI_SLIM): MIN + UI, backboardd, SpringBoard and
# PosterBoard among them
if [ -z "${A64_JOBS:-}" ]; then
	A64_JOBS=$(python3 -I - "$X/cli/libexec/arm64/boot-a64.sh" "${A64_JOB_LIST:-std}" <<-'PY'
	import re, sys
	s = open(sys.argv[1]).read()
	jobs = []
	for name in ("MIN", "UI_SLIM" if sys.argv[2] == "slim" else "UI"):
	    jobs += re.search(r'^%s="(.*?)"' % name, s, re.S | re.M).group(1).split()
	print(" ".join(jobs))
	PY
	)
	# x86Sim's in-device helper for simctl's commands (cli/libexec/simctl.py launch/listapps/..., openurl)
	A64_JOBS="$A64_JOBS com.x86sim.openurl=$X/host/openurl/arm64/openurl"
	# x86Sim's xctest feature (cli/libexec/arm64/features-def.json): testmanagerd and the automation-mode jobs, for UI
	# test runners. launchd_sim starts them on demand, so they cost nothing until a UI test runs
	A64_JOBS="$A64_JOBS com.apple.testmanagerd com.apple.dt.automationmode-writer com.apple.dt.AutomationModeUI"
fi
# Metal: this boot's mtlvk (Vulkan; Mesa's lavapipe without a GPU) on its own socket, as boot-a64.sh starts it
ST=${X86SIM_STATE:-$X/var/run}
mkdir -p "$ST"
MTLVK_BIN=${MTLVK:-$X/linux/mtlvk/target/release/mtlvk}
if [ -n "$X86SIM_METAL" ] && [ "$X86SIM_METAL" != 0 ]; then
	export X86SIM_MTLVK_SOCKET=$ST/mtlvk.sock
	[ -f "$ST/mtlvk.pid" ] && kill "$(cat "$ST/mtlvk.pid")" 2>/dev/null || true
	rm -f "$ST/mtlvk.sock"
	if [ -x "$MTLVK_BIN" ]; then
		(setsid "$MTLVK_BIN" "$ST/mtlvk.sock" > "$ST/mtlvk.log" 2>&1 < /dev/null & echo $! > "$ST/mtlvk.pid")
		for i in $(seq 1 50); do [ -S "$ST/mtlvk.sock" ] && break; sleep 0.1; done
	fi
	if [ -S "$ST/mtlvk.sock" ]; then
		echo "boot-arm64: mtlvk up (pid $(cat "$ST/mtlvk.pid")), Metal for: $X86SIM_METAL" >&2
	else
		echo "boot-arm64: mtlvk did not start ($MTLVK_BIN, see $ST/mtlvk.log): no Metal, nothing will be drawn" >&2
		export X86SIM_METAL=0
	fi
fi
# Audio: the simulator's audio devices (libsimaudio in each process) are served by simaudio.py, as a Mac's coreaudiod
# serves them; without it CoreAudio has no system object and SpringBoard never finishes starting (the lock screen
# never comes up). No PipeWire playback by default (a container has no sound).
if [ "${X86SIM_AUDIO_SOCKET:-}" != none ]; then
	export X86SIM_AUDIO_SOCKET=${X86SIM_AUDIO_SOCKET:-$ST/simaudio.sock}
	[ -f "$ST/simaudio.pid" ] && kill "$(cat "$ST/simaudio.pid")" 2>/dev/null || true
	rm -f "$X86SIM_AUDIO_SOCKET"
	(X86SIM_AUDIO_PLAYBACK=${X86SIM_AUDIO_PLAYBACK:-0} setsid python3 "$X/cli/libexec/simaudio.py" "$X86SIM_AUDIO_SOCKET" \
		> "$ST/simaudio.log" 2>&1 < /dev/null & echo $! > "$ST/simaudio.pid")
fi
# Apple's files may be read-only, from elsewhere: AGENT_SIM_RUNTIME_RO, the runtime's directory (RuntimeRoot,
# extracted/; default $X86SIM_ROOT/runtimes-ro/ios-27.2-24B8117-arm64), and AGENT_SIM_XCTEST_PLATFORM, Xcode's
# iPhoneSimulator.platform (default $X86SIM_ROOT/sdks/iPhoneSimulator.platform): a Bazel repository's extracted archive,
# or a read-only volume. x86Sim writes each device's launch angels into the runtime's own LaunchAngels directory
# (launchd_sim takes a job as an angel only from there), so a read-only runtime gets an overlay at
# $X86SIM_ROOT/runtimes/ios-27.2-24B8117-arm64, the read-only files below and this machine's writes above. Once per
# machine, whichever phone boots first.
RUNTIME_NAME=ios-27.2-24B8117-arm64
RUNTIME_RO=${AGENT_SIM_RUNTIME_RO:-$X/runtimes-ro/$RUNTIME_NAME}
mkdir -p "$X/var"
if [ -d "$RUNTIME_RO" ]; then
	(
		flock 9
		if ! mountpoint -q "$X/runtimes/$RUNTIME_NAME"; then
			mkdir -p "$X/runtimes/$RUNTIME_NAME" "$X/var/overlay/$RUNTIME_NAME/upper" "$X/var/overlay/$RUNTIME_NAME/work"
			mount -t overlay overlay -o "lowerdir=$(readlink -f "$RUNTIME_RO"),upperdir=$X/var/overlay/$RUNTIME_NAME/upper,workdir=$X/var/overlay/$RUNTIME_NAME/work" \
				"$X/runtimes/$RUNTIME_NAME"
		fi
	) 9> "$X/var/.runtime-overlay.lock"
fi
# XCTest's platform where agent-sim xctest and x86Sim's runner look for it
if [ -n "${AGENT_SIM_XCTEST_PLATFORM:-}" ] && [ ! -e "$X/sdks/iPhoneSimulator.platform" ]; then
	mkdir -p "$X/sdks"
	ln -s "$(readlink -f "$AGENT_SIM_XCTEST_PLATFORM")" "$X/sdks/iPhoneSimulator.platform"
fi
python3 -I "$X/cli/libexec/mksimdevice.py" "$D" $A64_JOBS > /dev/null
# agent-sim's hooks (hooks/prebuilt, simulator libraries) in SpringBoard: the status bar override (`simctl status_bar`)
# and the accessibility reader. A native boot has no x86Sim per-program environment file (its translator's): SpringBoard's
# job gets them as X86SIM_A64_GENV_DYLD_INSERT_LIBRARIES, which Darling's dyld applies (patches/darling-arm64), after
# the libraries every process gets. AGENT_SIM_HOOKS=0 leaves them out.
HOOKS=$(cd "$HERE/../hooks/prebuilt" 2>/dev/null && pwd || true)
if [ "${AGENT_SIM_HOOKS:-1}" = 1 ] && [ -f "$HOOKS/libagentsim-ax.dylib" ]; then
	python3 -I - "$D" "/Volumes/SystemRoot$HOOKS" <<-'PY'
	import plistlib, sys
	d, h = sys.argv[1:3]
	env = dict(l.rstrip("\n").split("=", 1) for l in open(d + "/sim.env") if "=" in l)
	libs = [x for x in env.get("DYLD_INSERT_LIBRARIES", "").split(":") if x]
	libs += [h + "/libagentsim-ax.dylib", h + "/libagentsim-springboard.dylib"]
	p = d + "/LaunchDaemons/com.apple.SpringBoard.plist"
	job = plistlib.load(open(p, "rb"))
	job.setdefault("EnvironmentVariables", {})["X86SIM_A64_GENV_DYLD_INSERT_LIBRARIES"] = ":".join(libs)
	plistlib.dump(job, open(p, "wb"))
	PY
fi
# App accessibility on, as Accessibility Inspector turns it on in Apple's simulator (and setup-vm.sh does on x86-64):
# without it a UI test runner waits for accessibility to load and gives up ("Timed out while loading Accessibility")
python3 -I - "$D/data/Library/Preferences/com.apple.Accessibility.plist" <<-'PY'
import os, plistlib, sys
p = sys.argv[1]
d = plistlib.load(open(p, "rb")) if os.path.exists(p) else {}
if not (d.get("ApplicationAccessibilityEnabled") and d.get("AccessibilityEnabled")):
    d.update(ApplicationAccessibilityEnabled=True, AccessibilityEnabled=True)
    os.makedirs(os.path.dirname(p), exist_ok=True)
    plistlib.dump(d, open(p, "wb"), fmt=plistlib.FMT_BINARY)
PY

UDID=$(sed -n 's/^SIMULATOR_UDID=//p' "$D/sim.env")
LOG=$DPREFIX/private/var/tmp/com.apple.CoreSimulator.SimDevice.$UDID/launchd.log
echo "boot-arm64: device $D ($UDID), launchd_sim's log: $LOG" >&2

ENVS=$(sed 's/^/"/; s/$/"/' "$D/sim.env" | tr '\n' ' ')
RT="/Volumes/SystemRoot$(readlink -f "$X/runtimes/$RUNTIME_NAME/RuntimeRoot")"
CMD="exec '/Volumes/SystemRoot$SIMHOST' /usr/bin/env $ENVS '$RT/sbin/launchd_sim' '/Volumes/SystemRoot$D/data/var/run/launchd_bootstrap.plist'"
if [ -n "${1:-}" ]; then
	exec timeout "$1" darling shell sh -c "$CMD" < /dev/null
else
	exec darling shell sh -c "$CMD" < /dev/null
fi
