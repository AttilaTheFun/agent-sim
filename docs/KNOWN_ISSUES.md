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
| A permission prompt stays on screen after the app that asked quits or crashes | Darling delivers a Mach dead-name notification within a process but loses it when the port's owner is another process that died. SpringBoard watches the prompt's reply port for that notification and tccd watches the asking app's connection the same way, so neither noticed | `hooks/springboard.c`, loaded into SpringBoard by `phone.sh`, checks each alert once a second and cancels it when the process that posted it, or the app a permission prompt names, is gone. tccd records the canceled prompt as "denied, prompt canceled" (reason 13); reset with `x86sim privacy reset` |
| Built-in apps' permission prompts get in the way of other apps | a prompt stays up over every app until answered, as on iOS | `setup-vm.sh` grants the built-in apps their permissions up front, as on a phone that has been set up; apps you install still ask |

## Open

### Lost dead-name notifications (root cause, in Darling)

When a process dies, Darling turns the send rights other processes hold to its ports into dead names, but the
dead-name notifications they asked for never arrive (a test inside one process gets both the raw notification and
libdispatch's `DISPATCH_MACH_SEND_DEAD` event). Anything that relies on noticing a peer's death across processes, such
as an XPC connection's interruption, is affected. The SpringBoard hook covers permission prompts; the fix belongs in
Darling's darlingserver.

Note that a permission prompt staying over other apps while its app is still running is iOS behavior, not this
bug: SpringBoard ignores Home while such an alert is up ("an alert item or sheet is showing and can't be dismissed").

### News stays black

News opens but never draws. Its main thread loops on signal delivery (about 1,500 `sigprocess` /
`interrupt_enter` / `interrupt_exit` / `thread_self_trap` calls a second through darlingserver) instead of reaching
its first frame. Adding every service of x86Sim's standard list does not change it, nor do `GIGACAGE_ENABLED=0` or
`JSC_useJIT=false` for the app.

### Siri

The Siri app closes when opened directly; x86Sim's own Siri UI runs as `com.apple.campo` instead.

### Maps shows no map tiles

The map data comes from a Mac and is not part of the iOS runtime (x86Sim limitation).
