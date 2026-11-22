# Benchmarks

`bench/run` measures one phone: boot time, app launches (first frame and settled screen, median of 3), scrolling a
list in Settings (median of 3; a run counts only when Settings is in front and the list moved), touch-to-frame
latency, CPU at rest and memory. It uses x86Sim's own `tests/perf.py` for the measurements and writes JSON.

All numbers: x86Sim 0.5.2, iOS 27.2 runtime (24B8117), 8 vCPU / 16 GB cloud VMs without a GPU, Ubuntu 24.04.

## Current setup, by CPU

The phone as `scripts/setup-vm.sh` and `scripts/phone.sh` set it up: iPhone 17 Pro drawn at 2x, pre-translated
runtime, x86Sim's slim services, Metal on Mesa's CPU Vulkan driver, Reduce Transparency and Reduce Motion on.

| | AMD EPYC Zen 5 (9455) | AMD EPYC Zen 3 (Milan) |
|---|---|---|
| New phone's first boot | 112 s | about 6.5 min |
| Boot to home screen | 13–17 s | 20–22 s |
| App launch, first frame | 0.4–0.7 s | 0.8–1.4 s |
| App launch, settled (Settings, Safari) | 2.2–2.4 s | 2.3–2.9 s |
| Settings scrolling | **53 fps** (frame p50 16.7 ms, p95 24.7 ms) | 21–22 fps |
| Touch to first new frame (p50) | 64 ms | 21–34 ms |
| Memory in use | 2.8 GB | 1.6–2.0 GB |

The Zen 3 column predates `bench/run` and the last two settings (software drawing, single runs), so it is a lower
bound. Per core speed matters most: the same 8 vCPUs on Zen 5 boot a new phone 3.5 times faster.

## What each step bought (Zen 5, `bench/run`)

| Setup | Settings scroll | Touch to frame | Settled launch | Boot |
|---|---|---|---|---|
| Software drawing, slim services, 2x | 39.8 fps | 88 ms | 3.3–3.6 s | 13.4 s |
| + Reduce Transparency and Motion | 47.4 fps | 80 ms | 3.3–3.6 s | 13.5 s |
| + Metal on lavapipe, 6 threads | **53.1 fps** | **64 ms** | **2.2–2.4 s** | 17.1 s |

## Earlier steps (Zen 3, single runs)

| Setup | Boot to home | Settings scroll | Touch to frame (p50) | Memory |
|---|---|---|---|---|
| iPhone 17 Pro, Metal on lavapipe, no pre-translation | 42 s | 40 fps | 475 ms | 5.7 GB |
| + pre-translated runtime | 34 s | 24 fps | 259 ms | 2.7 GB |
| software drawing instead of Metal | 32 s | 14 fps | 27–68 ms | 2.2 GB |
| + slim background services | 22 s | 14 fps | 27–68 ms | 1.6 GB |
| + iPhone 17 Pro (at 2x) | 20–22 s | 21–22 fps | 21–34 ms | 1.6 GB |

## What the numbers say

- **Pre-translation** halves memory and boot time: without it every app translates the system frameworks' ARM
  code on first use.
- **Drawing is the bottleneck for motion.** Software drawing renders every frame on the CPU as translated code;
  frame rate follows pixel count (the 2x device scrolls about 1.5 times faster than 3x). Blur and motion effects
  cost about 20%. Metal on lavapipe moves drawing into native code and is fastest once the effects are off.
- **CPU generation matters more than anything else tried.** Zen 5 roughly doubles boot, launch and scroll speed
  over Zen 3 at the same core count.
- Turning off more services than x86Sim's `slim` list (Siri, Spotlight, iCloud, Home, Wallet) did not help.

## Streaming (`agent-sim serve`)

`agent-sim encode-bench` times the browser stream on synthetic scrolling frames (and scores quality by decoding
them again). For the 2x phone at full size (804×1748):

| | Before | After |
|---|---|---|
| Color conversion, Apple M-series | 10.7 ms | 0.7 ms (specialized per scale, rows on 4 threads) |
| H.264 encode, Apple M-series | 10.4 ms | 3.7 ms (OpenH264 camera mode) |
| Quality (luma PSNR) | 43.5 dB | 54.3 dB |
| Conversion + encode, Zen 5 VM | | 0.9 + 8.9 ms |

## Not measured yet

- Idle CPU: SpringBoard and darlingserver use about a core between them with nothing on screen.
- Pinning the drawing threads to their own cores; Linux 6.11+.
- End-to-end touch-to-pixels in a browser.
