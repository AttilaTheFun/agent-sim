# Known issues

What does not work yet, what we know about the cause, and what agent-sim does about it. Issues whose fix belongs in
x86Sim are reported there with the evidence below.

## Fixed or worked around in agent-sim

| Issue | Cause | What agent-sim does |
|---|---|---|
| Every web page crashes ("This webpage was reloaded because a problem occurred") | WebKit's web content process reserves a 16 GB "Gigacage" at startup; the reservation fails under x86Sim (`FATAL: Could not allocate gigacage memory with maxAlignment = 17179869184`) | `phone.sh` sets WebKit's own switch, `GIGACAGE_ENABLED=0`, for `com.apple.WebKit.WebContent` through x86Sim's per-program environment file (`~/.cache/x86sim/a64-penv`) |
| More than a core busy with nothing on screen | x86Sim's `slim` service list leaves out `com.apple.sleepd`; SpringBoard retries reaching it about 780 times a second (each try a burst of `mach_port_construct`/`mach_msg` calls through darlingserver) | `phone.sh` adds sleepd back |
| Photos closes at once; Wallet and Watch stay black | services they depend on are not in `slim` | `phone.sh` adds back assetsd, passd, financed, nanoregistryd, accountsd, amsaccountsd, mediaremoted, sharingd and medialibraryd |
| Pages that start a web worker or service worker crash ("A problem repeatedly occurred") | WebKit asks for a new thread's stack bounds from the creating thread; x86Sim described the child's native stack, not the guest stack it runs on, and JavaScriptCore's `StackBounds::contains` check failed in `sanitizeStackForVM` | `patches/x86sim/0001-guest-stacks-of-new-threads.patch`, built by `scripts/patch-x86sim.sh` during setup |
| Settings shows gray or blank row icons | Settings finds no rendered icon in the shared icon store and shows a placeholder; the store stays empty because iconservicesagent crashes rendering them: CoreUI force-unwraps `sharedCIContext`, which is nil when the process has no Metal device (x86Sim gives Metal only to the processes named in `X86SIM_METAL`) | `phone.sh` gives iconservicesagent Metal too (`X86SIM_METAL=backboardd,iconservicesagent`). A phone that ran without it keeps the blank icons it cached until its icon store is rebuilt (delete `data/Containers/Shared/SystemGroup/systemgroup.com.apple.lsd.iconscache/Library/Caches/com.apple.IconsCache` with the phone stopped) |
| Permission prompts stay on screen over other apps | see below | `setup-vm.sh` grants the built-in apps their permissions up front, as on a phone that has been set up; apps you install still ask |

## Open

### Permission prompts are not dismissed when their app leaves

On a device, SpringBoard cancels an app's permission alert when the requesting process's reply port dies, and
holds it back while the app is not in front. Under x86Sim neither happens, so a prompt from one app stays over the
next. Likely a missing dead-name notification or display-layout update in Darling.

### News stays black

News opens but never draws. Its main thread loops on signal delivery (about 1,500 `sigprocess` /
`interrupt_enter` / `interrupt_exit` / `thread_self_trap` calls a second through darlingserver) instead of reaching
its first frame. Adding every service of x86Sim's standard list does not change it, nor do `GIGACAGE_ENABLED=0` or
`JSC_useJIT=false` for the app.

### Siri

The Siri app closes when opened directly; x86Sim's own Siri UI runs as `com.apple.campo` instead.

### Maps shows no map tiles

The map data comes from a Mac and is not part of the iOS runtime (x86Sim limitation).
