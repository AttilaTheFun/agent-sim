# Benchmarks

Measured with x86Sim's own suite (`tests/perf.py`): launch times from `openurl` to the first frame and to a screen
that stays still for a second, scrolling a list in Settings for 8 seconds, and the time from a touch's first move to
the first new frame published. Each row is the best valid run of one to three runs; runs where Settings was not in
front were thrown out. Numbers will move to repeated runs once `bench/` exists.

## Machine

A cloud VM: 8 vCPU AMD EPYC Zen 3 (Milan), 16 GB, no GPU, Ubuntu 24.04, Linux 6.1. x86Sim 0.5.2, iOS 27.2 runtime
(24B8117).

## One device, step by step

| Setup | Boot to home | Launch, first frame / settled | Settings scroll | Touch to frame (p50) | VM memory |
|---|---|---|---|---|---|
| iPhone 17 Pro, Metal on lavapipe, no pre-translation | 42 s | Safari 0.2 / 1.3 s, Settings 3.7 s | 40 fps | 475 ms | 5.7 GB |
| + pre-translated runtime | 34 s | Settings 1.8 / 2.8 s, Maps 1.8 / 2.2 s | 24 fps | 259 ms | 2.7 GB |
| software drawing instead of Metal | 32 s | Settings 1.5 / 2.6 s, News 1.2 / 2.3 s | 14 fps | 27–68 ms | 2.2 GB |
| + slim background services | 22 s | about 1.1–1.8 / 2.3 s | 14 fps | 27–68 ms | 1.6 GB |
| **+ iPhone 17 Pro (at 2x)** | **20–22 s** | **0.8–1.4 / 2.3–2.9 s** | **21–22 fps** | **21–34 ms** | **1.6 GB** |

The last row is what `scripts/setup-vm.sh` and `scripts/phone.sh` set up.

## What the numbers say

- **Pre-translation** halves memory and cuts boot time: without it every app translated the system frameworks'
  ARM code on first use.
- **Drawing is the bottleneck for motion.** With software drawing, backboardd renders every frame on the CPU, as
  translated code, on about 2.5 cores. Frame rate follows pixel count: the 2x device (44% of the pixels) scrolls
  about 1.5 times faster than the 3x one and copies each frame in 2.8 ms instead of 9.6 ms.
- **Touches are answered quickly** once drawing is cheap enough: 21–34 ms to the first new frame.
- **App launches settle at about 2.3 s** on every setup tried. The time is spent in the launching app and the system
  processes around it, not in drawing.
- Turning off more services than x86Sim's `slim` list (Siri, Spotlight, iCloud, Home, Wallet) did not help.

## Not yet measured properly

- Metal on lavapipe against software drawing at 2x, with Settings in front.
- Reduce Transparency and Reduce Motion (blur is expensive to draw on a CPU).
- A faster CPU (Zen 5), and pinning the drawing threads to their own cores.
- Browser viewer latency end to end; `agent-sim serve` encodes a frame in about 14–45 ms on this machine.
