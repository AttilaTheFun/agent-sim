#!/bin/bash
# Boots Apple's iOS simulator runtime natively on arm64 Linux, under Darling built with patches/darling-arm64 (no
# translator: the runtime's arm64 code runs as it is). x86Sim provides the device (cli/libexec/mksimdevice.py), the
# programs that stand in for CoreSimulator and the Mac inside the simulator (host/*/arm64, prebuilt) and simhost,
# which plays CoreSimulator's part of the launchd handshake (built for arm64 by scripts/build-simhost-arm64.sh).
#
#   scripts/boot-arm64.sh [seconds]      (default: until stopped)
#
# Env: X86SIM_ROOT (an x86Sim checkout with the runtime fetched; default ~/x86sim), A64_DEVICE (default
# $X86SIM_ROOT/var/device-a64), A64_JOBS (default: boot-a64.sh's slim list, MIN + UI_SLIM), DPREFIX (Darling prefix,
# default ~/.darling), X86SIM_METAL (default all: Metal for every process through x86Sim's metalhostd and mtlvk, built
# for arm64 by scripts/build-mtlvk-arm64.sh; 0 = none, and backboardd then draws nothing). Frames go to simfb's ring
# (SIMFB_RING in sim.env; x86Sim's cli/libexec/fb2png.py makes a PNG of the newest).
set -e
X=${X86SIM_ROOT:-$HOME/x86sim}
export X86SIM_ROOT=$X X86SIM_PREFIX=${X86SIM_PREFIX:-/usr/local} X86SIM_METAL=${X86SIM_METAL-all}
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
python3 -I "$X/cli/libexec/mksimdevice.py" "$D" $A64_JOBS > /dev/null

UDID=$(sed -n 's/^SIMULATOR_UDID=//p' "$D/sim.env")
LOG=$DPREFIX/private/var/tmp/com.apple.CoreSimulator.SimDevice.$UDID/launchd.log
echo "boot-arm64: device $D ($UDID), launchd_sim's log: $LOG" >&2

ENVS=$(sed 's/^/"/; s/$/"/' "$D/sim.env" | tr '\n' ' ')
RT="/Volumes/SystemRoot$(readlink -f "$X/runtimes/ios-27.2-24B8117-arm64/RuntimeRoot")"
CMD="exec '/Volumes/SystemRoot$SIMHOST' /usr/bin/env $ENVS '$RT/sbin/launchd_sim' '/Volumes/SystemRoot$D/data/var/run/launchd_bootstrap.plist'"
if [ -n "${1:-}" ]; then
	exec timeout "$1" darling shell sh -c "$CMD" < /dev/null
else
	exec darling shell sh -c "$CMD" < /dev/null
fi
