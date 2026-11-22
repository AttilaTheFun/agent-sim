#!/usr/bin/env bash
# Start, stop or wait for an x86Sim phone with agent-sim's settings.
#
#   scripts/phone.sh start [DEVICE]    boot DEVICE (default p) in the background
#   scripts/phone.sh wait [DEVICE]     block until its home screen is up (prints the seconds it took)
#   scripts/phone.sh stop [DEVICE]
#   scripts/phone.sh status [DEVICE]
#
# Settings, each overridable from the environment (docs/BENCHMARKS.md has the measurements):
#   X86SIM_METAL=backboardd   Metal on Mesa's CPU Vulkan driver (lavapipe): smoother scrolling than software drawing
#   LP_NUM_THREADS            lavapipe's threads: two fewer than the CPUs, 2 to 6
#   A64_JOBS=slim             x86Sim's smaller list of background services
# The frame ring lives under ~/.cache rather than /run/user: x86Sim's container does not always see the host's
# /run/user (no login session on a server), and then no frame ever reaches the host.
set -Eeuo pipefail

ACTION=${1:?usage: phone.sh start|wait|stop|status [DEVICE]}
DEVICE=${2:-${DEVICE:-p}}
ROOT=${AGENT_SIM_HOME:-$HOME/.local/share/agent-sim}
X86SIM_VERSION=${X86SIM_VERSION:-0.5.2}
X=$ROOT/x86sim-$X86SIM_VERSION/AppRun
DATA=${XDG_DATA_HOME:-$HOME/.local/share}/x86sim
DIR=$DATA/var/device-a64-$DEVICE
RINGS=$HOME/.cache/x86sim/rings
RING=$RINGS/x86sim-a64-$DEVICE-simfb.ring
UNIT=agent-sim-phone-$DEVICE

[ -x "$X" ] || { echo "phone.sh: x86Sim is not installed; run scripts/setup-vm.sh" >&2; exit 1; }
[ -d "$DIR" ] || { echo "phone.sh: no device $DEVICE at $DIR" >&2; exit 1; }
export XDG_RUNTIME_DIR=${XDG_RUNTIME_DIR:-/run/user/$(id -u)}
export DBUS_SESSION_BUS_ADDRESS=${DBUS_SESSION_BUS_ADDRESS:-unix:path=$XDG_RUNTIME_DIR/bus}

stop_phone() {
	systemctl --user stop "$UNIT" 2>/dev/null || true
	"$X" shell -c "A64_DEVICE=$DIR A64_STOP_ONLY=1 cli/libexec/arm64/boot-a64.sh" >/dev/null 2>&1 || true
	systemctl --user reset-failed "$UNIT" 2>/dev/null || true
}

case $ACTION in
start)
	stop_phone
	mkdir -p "$RINGS"
	# a ring left from the last boot would look like a home screen already drawn
	rm -f "$RING" "$RING".* "$DIR/data/framebuffer.bgra"
	systemd-run --user --quiet --unit="$UNIT" --collect \
		--setenv=SIMFB_RING="/Volumes/SystemRoot$RING" \
		--setenv=X86SIM_METAL="${X86SIM_METAL:-backboardd}" \
		--setenv=LP_NUM_THREADS="${LP_NUM_THREADS:-$(n=$(($(nproc) - 2)); echo $((n < 2 ? 2 : n > 6 ? 6 : n)))}" \
		--setenv=A64_JOBS="${A64_JOBS:-slim}" \
		--setenv=VK_ICD_FILENAMES="${VK_ICD_FILENAMES:-/usr/share/vulkan/icd.d/lvp_icd.json}" \
		"$X" shell -c "A64_DEVICE=$DIR A64_KEEP=1 SECS=7200 cli/libexec/arm64/boot-a64.sh"
	echo "phone.sh: starting $DEVICE (journalctl --user -u $UNIT)"
	;;
wait)
	start=$(date +%s)
	until [ -e "$RING" ]; do
		systemctl --user is-active --quiet "$UNIT" || { echo "phone.sh: $DEVICE is not running" >&2; exit 1; }
		sleep 0.5
	done
	X86SIM_DEVICE=$DIR SIMFB_RING_LINUX=$RING "$X" x86sim wait home "${TIMEOUT:-900}" >/dev/null
	echo "phone.sh: $DEVICE home screen after $(($(date +%s) - start)) s"
	;;
stop)
	stop_phone
	;;
status)
	systemctl --user is-active "$UNIT" || true
	;;
*)
	echo "usage: phone.sh start|wait|stop|status [DEVICE]" >&2
	exit 2
	;;
esac
