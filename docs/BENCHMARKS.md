# Benchmarks

`bench/run` measures one phone: boot time, app launches (first frame and settled screen, median of 3), scrolling a
list in Settings (median of 3; a run counts only when Settings is in front and the list moved), touch-to-frame
latency, CPU at rest and memory. It uses x86Sim's own `tests/perf.py` for the measurements and writes JSON.

All numbers: x86Sim 0.5.2, iOS 27.2 runtime (24B8117), 8 vCPU / 16 GB cloud VMs without a GPU, Ubuntu 24.04.

## Current setup, by CPU

The phone as `scripts/setup-vm.sh` and `scripts/phone.sh` set it up: iPhone 17 Pro drawn at 2x, pre-translated
runtime, x86Sim's slim services plus the ones apps need, Metal on Mesa's CPU Vulkan driver, Reduce Transparency and
Reduce Motion on.

| | AMD EPYC Zen 5 (9455) | AMD EPYC Zen 3 (Milan) |
|---|---|---|
| New phone's first boot | 112 s | about 6.5 min |
| Boot to home screen | 12–13 s | 20–22 s |
| App launch, first frame | 0.4–0.7 s | 0.8–1.4 s |
| App launch, settled (Settings, Safari) | 2.2–2.4 s | 2.3–2.9 s |
| Settings scrolling | **54.7 fps** (frame p50 16.7 ms, p95 25.7 ms) | 21–22 fps |
| Touch to first new frame (p50) | 13.5 ms | 21–34 ms |
| CPU at rest | about 0 | |
| Memory in use | 2.7 GB | 1.6–2.0 GB |

The Zen 3 column predates `bench/run` and the last two settings (software drawing, single runs), so it is a lower
bound. Per core speed matters most: the same 8 vCPUs on Zen 5 boot a new phone 3.5 times faster.

## What each step bought (Zen 5, `bench/run`)

| Setup | Settings scroll | Touch to frame | Settled launch | Boot |
|---|---|---|---|---|
| Software drawing, slim services, 2x | 39.8 fps | 88 ms | 3.3–3.6 s | 13.4 s |
| + Reduce Transparency and Motion | 47.4 fps | 80 ms | 3.3–3.6 s | 13.5 s |
| + Metal on lavapipe, 6 threads | 53.1 fps | 64 ms | 2.2–2.4 s | 17.1 s |
| + sleepd and the services apps need | **54.7 fps** | **13.5 ms** | **2.2–2.4 s** | **12.5 s** |

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

Adding back `com.apple.sleepd` matters most for the last row. Without it SpringBoard keeps retrying and about 1.3 cores
stay busy with nothing on screen, which also slowed touches. With it the phone is idle at rest.

## Several phones on one VM (Zen 4c EPYC 8224P, 8 vCPU / 16 GB)

Three phones booted one after another; each one was checked to still draw by opening Settings after every boot.

| | Without KSM | With KSM (`MemoryKSM=yes`, 1000–2000 pages per scan) |
|---|---|---|
| Memory in use, VM idle | 1.7 GB | 1.7 GB |
| One phone | 4.5 GB | 4.7 GB |
| Three phones, 2 min after the last boot | **8.7 GB** | **6.8 GB** (2.2 GB of pages merged) |
| Load average, three phones at rest | 1.0 | 1.5 (KSM's own scanning) |
| Boot with other phones running | 29–32 s | 29–32 s |
| One phone at rest: CPU | 0.2–0.4 cores | 0.7 cores (about 0.4 of it is KSM scanning) |

This ran on a slower CPU than the tables above: the same VM, resumed, came back on an EPYC 8224P (Zen 4c), where one
phone boots in 25–31 s instead of 12–13 s on Zen 5. Memory does not depend on the CPU. Check `lscpu` before comparing
numbers across runs; a cloud provider may place the same VM type on different hardware.

Every phone kept drawing while the others booted. One phone needs about 2.3 GB on its own and about 1.7 GB with
KSM, so memory allows six phones on a 16 GB VM and about twelve on 32 GB. CPU sets the real limit: a phone at rest
costs nearly nothing, but a phone that is scrolling or animating keeps up to six drawing threads busy
(`LP_NUM_THREADS`); set it lower when many phones animate at once. KSM pays off only with several phones: with one
it just scans, so `scripts/pool.sh` turns it on when it starts two or more.

## Shader caches (Zen 4c EPYC 8224P)

Mesa (`~/.cache/mesa_shader_cache`) and metal2vulkan (`~/.cache/metal2vulkan`) keep compiled shaders between boots.

| Boot | Cache files written | Boot to home screen |
|---|---|---|
| Empty caches | 847 | 31 s |
| Second | 24 | 20 s |
| Third | 8 | 21 s |

Warm caches save about 10 s a boot. They live under the user's home, so every phone on the machine shares them, and
`setup-vm.sh`'s first boot fills them. A machine image or snapshot made after setup should keep `~/.cache`.

## Saved phones: VM memory snapshots

A phone cannot be checkpointed on its own (dozens of Darling processes, darlingserver, the Vulkan renderer and
their shared memory), but a VM provider that snapshots memory saves the whole machine with the phone running.
On Freestyle (AMD EPYC Milan, 8 vCPU / 16 GB), after a clean `setup-vm.sh` and a booted phone:

| | Time |
|---|---|
| Snapshot of the running VM | 3.8 s |
| New VM from the snapshot, first time | 78 s |
| New VM from the snapshot, again | **19 s** |
| Restored phone answers a launch (first new frame) | 2.0–2.5 s |

The restored phone was the same running phone (the VM's uptime carried on, no boot), and opened Settings with
its icons drawn. Compare a fresh VM: about half an hour of setup, then a boot. Boat's archive and resume keeps
the disk but not memory: the phone boots again, and the resumed VM can land on a different CPU.

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

- Scrolling performance per phone with several phones animating at once.
- Pinning the drawing threads to their own cores; Linux 6.11+.
- End-to-end touch-to-pixels in a browser.
