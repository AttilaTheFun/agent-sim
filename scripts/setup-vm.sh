#!/usr/bin/env bash
# Turn a fresh Ubuntu 24.04 x86-64 machine into one that runs an iOS phone with x86Sim, set up the way agent-sim
# measured fastest. Re-runnable: each step skips what is already done.
#
#   scripts/setup-vm.sh
#
# Environment:
#   X86SIM_VERSION   x86Sim release (default 0.5.2)
#   RUNTIME_BUILD    iOS runtime build from Apple's catalogue (default 24B8117, iOS 27.2)
#   DEVICE, MODEL    the phone to create (default p, "iPhone 17 Pro (at 2x)")
#   AGENT_SIM_PATCH_X86SIM=0   keep x86Sim's own translator (skips the fixes in patches/x86sim)
#
# Apple's iOS runtime is downloaded from Apple onto this machine (about 8 GB) and never leaves it.
set -Eeuo pipefail

X86SIM_VERSION=${X86SIM_VERSION:-0.5.2}
RUNTIME_BUILD=${RUNTIME_BUILD:-24B8117}
DEVICE=${DEVICE:-p}
MODEL=${MODEL:-"iPhone 17 Pro (at 2x)"}
ROOT=${AGENT_SIM_HOME:-$HOME/.local/share/agent-sim}
X=$ROOT/x86sim-$X86SIM_VERSION/AppRun
DATA=${XDG_DATA_HOME:-$HOME/.local/share}/x86sim
HERE=$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)

log() { printf '\n==> %s\n' "$*"; }
die() { printf 'setup-vm.sh: %s\n' "$*" >&2; exit 1; }

log "Checking the machine"
[ "$(uname -m)" = x86_64 ] || die "x86Sim needs an x86-64 machine"
glibc=$(ldd --version | head -1 | grep -oE '[0-9]+\.[0-9]+$')
awk "BEGIN{exit !($glibc >= 2.39)}" || die "x86Sim needs glibc 2.39 or newer (Ubuntu 24.04+), found $glibc"
free_gb=$(df -BG --output=avail "$HOME" | tail -1 | tr -dc 0-9)
[ "$free_gb" -ge 45 ] || [ -d "$DATA/runtimes" ] || die "need 45 GB free in $HOME for the runtime, have ${free_gb} GB"
[ "$(nproc)" -ge 4 ] || echo "warning: fewer than 4 CPUs; the phone will be slow"

log "Installing packages"
sudo DEBIAN_FRONTEND=noninteractive apt-get update -qq
sudo DEBIAN_FRONTEND=noninteractive apt-get install -yqq \
	python3 python3-pil python3-dnspython curl openssl jq mesa-vulkan-drivers >/dev/null

log "Keeping a user session for x86Sim's sockets and services"
# x86Sim puts sockets under /run/user/<uid> and runs phones as user services; without a login session (a server
# reached over SSH or an API) neither exists unless the user lingers.
sudo loginctl enable-linger "$USER"
for _ in $(seq 20); do [ -S "/run/user/$(id -u)/bus" ] && break; sleep 0.5; done
export XDG_RUNTIME_DIR=/run/user/$(id -u)
export DBUS_SESSION_BUS_ADDRESS=unix:path=$XDG_RUNTIME_DIR/bus

log "Letting the kernel merge identical memory pages between phones (KSM)"
# Phones run the same iOS runtime: with KSM three phones use 6.8 GB instead of 8.7 GB (docs/BENCHMARKS.md).
# phone.sh opts each phone in (MemoryKSM=yes); this turns the merging on, now and at every boot.
if [ -e /sys/kernel/mm/ksm/run ]; then
	printf 'w /sys/kernel/mm/ksm/run - - - - 1\nw /sys/kernel/mm/ksm/pages_to_scan - - - - 1000\n' |
		sudo tee /etc/tmpfiles.d/agent-sim-ksm.conf >/dev/null
	sudo systemd-tmpfiles --create /etc/tmpfiles.d/agent-sim-ksm.conf
else
	echo "setup-vm.sh: this kernel has no KSM; phones will not share memory" >&2
fi

if [ ! -x "$X" ]; then
	log "Downloading x86Sim $X86SIM_VERSION"
	tmp=$(mktemp -d)
	progress=-sS
	[ -t 1 ] && progress=--progress-bar
	curl -fL "$progress" -o "$tmp/x86sim.AppImage" \
		"https://github.com/x86sim/x86sim/releases/download/v$X86SIM_VERSION/x86sim-$X86SIM_VERSION-x86_64.AppImage"
	chmod +x "$tmp/x86sim.AppImage"
	# unpacked once, so it runs without FUSE
	(cd "$tmp" && ./x86sim.AppImage --appimage-extract >/dev/null)
	mkdir -p "$ROOT"
	rm -rf "$ROOT/x86sim-$X86SIM_VERSION"
	mv "$tmp/squashfs-root" "$ROOT/x86sim-$X86SIM_VERSION"
	rm -rf "$tmp"
fi
"$X" version

if [ "${AGENT_SIM_PATCH_X86SIM:-1}" = 1 ]; then
	log "Building x86Sim's translator with agent-sim's fixes (patches/x86sim; web workers in Safari)"
	# before pre-translating: the pre-translated image is tied to the translator build
	"$HERE/patch-x86sim.sh"
fi

fetched=$DATA/runtimes/.agent-sim-fetched-$RUNTIME_BUILD
if [ ! -e "$fetched" ]; then
	log "Downloading the iOS runtime $RUNTIME_BUILD from Apple (about 8 GB)"
	# Apple's CDN drops long downloads now and then; the fetch resumes where it stopped
	for attempt in 1 2 3 4 5; do
		if "$X" shell -c "python3 cli/libexec/arm64/runtime-fetch.py fetch $RUNTIME_BUILD"; then
			touch "$fetched"
			break
		fi
		[ "$attempt" = 5 ] && die "the runtime download failed 5 times"
		echo "download interrupted; resuming (attempt $((attempt + 1)) of 5)"
		sleep 5
	done
fi

log "Adding iPhone 17 Pro (at 2x): the iPhone 17 Pro's layout at 44% of the pixels"
types=$(ls "$DATA"/app/"$X86SIM_VERSION"*/cli/lib/devicetypes-a64.json | head -1)
python3 - "$types" <<'PY'
import json, sys
path = sys.argv[1]
types = json.load(open(path))
if "iPhone 17 Pro (at 2x)" not in types:
    t = dict(types["iPhone 17 Pro"])
    # the same 402x874 points: 2x instead of 3x, and the pitch with it
    t.update(w="804", h="1748", scale="2.000000", pitch="306.666667")
    types["iPhone 17 Pro (at 2x)"] = t
    json.dump(types, open(path, "w"), indent=1)
PY

if [ ! -d "$DATA/var/device-a64-$DEVICE" ]; then
	log "Creating the phone $DEVICE ($MODEL)"
	"$X" x86sim device create --model "$MODEL" "$DEVICE"
fi

aot_status() {
	local s
	s=$("$X" aot --device "$DATA/var/device-a64-$DEVICE" status 2>/dev/null) || true
	grep -m1 '^image ' <<<"$s" || true
}
DIR=$DATA/var/device-a64-$DEVICE
trained=$DIR/.agent-sim-trained
calm=$DIR/.agent-sim-reduced-effects
RUNTIME_ROOT=$(ls -d "$DATA"/runtimes/*-"$RUNTIME_BUILD"-arm64/extracted/Library/Developer/CoreSimulator/Profiles/Runtimes/*.simruntime/Contents/Resources/RuntimeRoot | head -1)
# Reduce Transparency and Reduce Motion: blurs and long animations are the costliest things to draw on a CPU
reduce_effects() {
	for key in ReduceMotionEnabled EnhancedBackgroundContrastEnabled; do
		(cd "$DATA" && X86SIM_DEVICE=$DIR "$X" x86sim spawn "$RUNTIME_ROOT/usr/bin/defaults" \
			write com.apple.Accessibility "$key" -bool true >/dev/null)
	done
	touch "$calm"
}
granted=$DIR/.agent-sim-granted
# The built-in apps get their permissions up front, as on a phone that has been set up: otherwise their prompts
# (location for Maps, contacts for Wallet...) show up, and x86Sim leaves them on screen over other apps. Apps you
# install still ask.
grant_builtin() {
	local apps
	apps=$(python3 - "$DIR/data/Library/SpringBoard/IconState.plist" <<'PY'
import plistlib, sys
out = []
def walk(v):
    if isinstance(v, str) and v.startswith("com.apple.") and v not in out:
        out.append(v)
    elif isinstance(v, (list, tuple)):
        for i in v: walk(i)
    elif isinstance(v, dict):
        for i in v.values(): walk(i)
walk(plistlib.load(open(sys.argv[1], "rb")))
print(" ".join(out))
PY
)
	for app in $apps; do
		(cd "$DATA" && X86SIM_DEVICE=$DIR "$X" x86sim privacy grant all "$app" >/dev/null 2>&1) || true
	done
	touch "$granted"
}
if [[ $(aot_status) != *"(current)"* ]] || [ ! -e "$calm" ] || [ ! -e "$granted" ]; then
	if [ ! -e "$trained" ] || [ ! -e "$calm" ] || [ ! -e "$granted" ]; then
		log "Booting the phone once: it learns what runs, gets Reduce Transparency and Motion, and its apps' permissions"
		"$HERE/phone.sh" start "$DEVICE"
		TIMEOUT=1800 "$HERE/phone.sh" wait "$DEVICE"
		reduce_effects
		grant_builtin
		# a first phone runs a while so the pre-translation learns what it uses; later phones reuse that image
		[ -e "$trained" ] || [[ $(aot_status) == *"(current)"* ]] || sleep 120
		"$HERE/phone.sh" stop "$DEVICE"
		touch "$trained"
	fi
	log "Pre-translating the runtime's code, so apps do not translate it as they run"
	# the first pass can make more code worth translating: repeat until the image is current
	for _ in 1 2 3; do
		"$X" aot --device "$DATA/var/device-a64-$DEVICE" update
		[[ $(aot_status) == *"(current)"* ]] && break
	done
	aot_status
fi

log "Done. Start the phone with:  scripts/phone.sh start $DEVICE && scripts/phone.sh wait $DEVICE"
