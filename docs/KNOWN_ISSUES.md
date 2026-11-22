# Known issues

What does not work yet, what we know about the cause, and what agent-sim does about it. Issues whose fix belongs in
x86Sim are reported there with the evidence below.

## Fixed or worked around in agent-sim

| Issue | Cause | What agent-sim does |
|---|---|---|
| Every web page crashes ("This webpage was reloaded because a problem occurred") | WebKit's web content process reserves a 16 GB "Gigacage" at startup; the reservation fails under x86Sim (`FATAL: Could not allocate gigacage memory with maxAlignment = 17179869184`) | `phone.sh` sets WebKit's own switch, `GIGACAGE_ENABLED=0`, for `com.apple.WebKit.WebContent` through x86Sim's per-program environment file (`~/.cache/x86sim/a64-penv`) |
| More than a core busy with nothing on screen | x86Sim's `slim` service list leaves out `com.apple.sleepd`; SpringBoard retries reaching it about 780 times a second (each try a burst of `mach_port_construct`/`mach_msg` calls through darlingserver) | `phone.sh` adds sleepd back |
| Photos closes at once; Wallet and Watch stay black | services they depend on are not in `slim` | `phone.sh` adds back assetsd, passd, financed, nanoregistryd, accountsd, amsaccountsd, mediaremoted, sharingd and medialibraryd |
| Permission prompts stay on screen over other apps | see below | `setup-vm.sh` grants the built-in apps their permissions up front, as on a phone that has been set up; apps you install still ask |

## Open

### Web workers and service workers crash the page

Pages that start a worker (many modern sites) end with "A problem repeatedly occurred". JavaScriptCore stops in
`sanitizeStackForVM` when it creates the worker thread's VM: the check `StackBounds::contains` fails because the
thread's recorded stack does not contain its stack pointer (recorded origin `0x7fffd10fc000`, stack pointer
`0x7fff47bff6e0`). Turning the JIT off (`JSC_useJIT=false`) does not change it.

x86Sim runs each guest thread on its own stack and answers `pthread_get_stackaddr_np` / `pthread_get_stacksize_np`
for it in `linux/arm64jit/handlers.cpp` only when `on_own_stack()` holds; otherwise the answer is Darling's, which
describes the native stack the guest does not run on. The fix belongs there (x86Sim).

### Settings shows gray row icons

Known to x86Sim (`docs/WORKAROUNDS.md`: "in-process IconServices store lookup misses"). Leads from x86Sim's notes:
the icon store is wiped unless its `__cache_info__` matches the build and its directory belongs to `_iconservices`,
and the LaunchServices database being rebuilt at boot may invalidate it.

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
