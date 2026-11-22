// SpringBoard: cancel a user-notification alert (permission prompts and other CFUserNotifications) once the process
// that posted it is gone, as SpringBoard does on a device when the alert's reply port dies. Under x86Sim the
// dead-name notification behind -[SBUserNotificationAlert _portWatcher] never arrives, so such alerts stayed up.
// A permission prompt (posted by tccd) also goes away when the app it asks for quits, as tccd cancels it on a device
// when its client's connection dies (the same lost notification). The app is the one the prompt names.
//
// Loaded into SpringBoard by scripts/phone.sh (x86Sim's per-program environment file). AGENT_SIM_HOOK_LOG=1 logs to
// /tmp/agent-sim-springboard.log on the host (=2 also logs every check).
#include <objc/runtime.h>
#include <objc/message.h>
#include <mach-o/dyld.h>
#include <mach/mach.h>
#include <dispatch/dispatch.h>
#include <signal.h>
#include <errno.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>

static FILE *lg;
static int verbose;
#define L(...) do { if (lg) { fprintf(lg, __VA_ARGS__); fflush(lg); } } while (0)

#define MAXA 64
static id alerts[MAXA];
static int app_pid[MAXA];   // the running app a tccd prompt asks for, or 0   // strong refs (retained via objc_retain) until cleaned up
static IMP orig_init;

#define MSG(r, o, sel, ...) ((r (*)(id, SEL, ##__VA_ARGS__))objc_msgSend)(o, sel_registerName(sel)
static id nsstr(const char *c) { return MSG(id, (id)objc_getClass("NSString"), "stringWithUTF8String:", const char *), c); }
static const char *cstr(id o) { return o ? MSG(const char *, o, "UTF8String")) : NULL; }
// tccd's prompt header names the app in curly quotes ("Allow “Wallet” to access your contacts?"): the pid of that
// app if it is running
static int prompt_app_pid(id msg) {
	const char *src = cstr(MSG(id, msg, "objectForKey:", id), nsstr("AlertSource")));
	const char *hdr = cstr(MSG(id, msg, "objectForKey:", id), nsstr("AlertHeader")));
	if (!src || strcmp(src, "tccd") || !hdr) return 0;
	const char *a = strstr(hdr, "\xE2\x80\x9C"), *b = a ? strstr(a + 3, "\xE2\x80\x9D") : NULL;
	if (!a || !b) return 0;
	char name[256]; size_t n = b - (a + 3);
	if (n >= sizeof name) return 0;
	memcpy(name, a + 3, n); name[n] = 0;
	id ctl = MSG(id, (id)objc_getClass("SBApplicationController"), "sharedInstance"));
	id apps = MSG(id, ctl, "runningApplications"));
	unsigned long count = apps ? MSG(unsigned long, apps, "count")) : 0;
	for (unsigned long i = 0; i < count; i++) {
		id app = MSG(id, apps, "objectAtIndex:", unsigned long), i);
		const char *dn = cstr(MSG(id, app, "displayName")));
		if (dn && !strcmp(dn, name)) {
			id st = MSG(id, app, "processState"));
			return st ? MSG(int, st, "pid")) : 0;
		}
	}
	return 0;
}
static ptrdiff_t off_pid = -1, off_port = -1, off_cleaned = -1, off_sent = -1;

extern id objc_retain(id);
extern void objc_release(id);

static id hook_init(id self, SEL s, id msg, unsigned port, unsigned flags, id token, id appc) {
	id r = ((id (*)(id, SEL, id, unsigned, unsigned, id, id))orig_init)(self, s, msg, port, flags, token, appc);
	if (r) {
		int app = msg ? prompt_app_pid(msg) : 0;
		for (int i = 0; i < MAXA; i++) if (!alerts[i]) { alerts[i] = objc_retain(r); app_pid[i] = app; break; }
		L("alert %p pid %d port %u app pid %d\n", r, off_pid >= 0 ? *(int *)((char *)r + off_pid) : -1, port, app);
		if (verbose && msg) {
			id d = ((id (*)(id, SEL))objc_msgSend)(msg, sel_registerName("description"));
			L("  message: %s\n", ((const char *(*)(id, SEL))objc_msgSend)(d, sel_registerName("UTF8String")));
		}
	}
	return r;
}
static void check(void *ctx) {
	for (int i = 0; i < MAXA; i++) {
		id a = alerts[i];
		if (!a) continue;
		char *p = (char *)a;
		int pid = *(int *)(p + off_pid);
		mach_port_t port = *(unsigned *)(p + off_port);
		bool done = *(bool *)(p + off_cleaned) || *(bool *)(p + off_sent);
		if (done) { alerts[i] = nil; app_pid[i] = 0; objc_release(a); continue; }
		mach_port_type_t t = 0;
		kern_return_t kr = port ? mach_port_type(mach_task_self(), port, &t) : KERN_INVALID_NAME;
		bool port_dead = kr != KERN_SUCCESS || (t & MACH_PORT_TYPE_DEAD_NAME);
		bool pid_gone = pid > 0 && kill(pid, 0) != 0 && errno == ESRCH;
		bool app_gone = app_pid[i] > 0 && kill(app_pid[i], 0) != 0 && errno == ESRCH;
		if (verbose) L("check %p pid %d gone %d port %u kr %d type %#x\n", a, pid, pid_gone, port, kr, t);
		if (port_dead || pid_gone || app_gone) {
			L("cancel %p (pid %d gone %d, port dead %d, app %d gone %d)\n", a, pid, pid_gone, port_dead, app_pid[i], app_gone);
			((void (*)(id, SEL))objc_msgSend)(a, sel_registerName("cancel"));
			alerts[i] = nil; app_pid[i] = 0;
			objc_release(a);
		}
	}
}
static void added(const struct mach_header *mh, intptr_t slide) {
	if (orig_init) return;
	Class c = objc_getClass("SBUserNotificationAlert");
	if (!c) return;
	Ivar v;
	if ((v = class_getInstanceVariable(c, "_originatingPID"))) off_pid = ivar_getOffset(v);
	if ((v = class_getInstanceVariable(c, "_replyPort"))) off_port = ivar_getOffset(v);
	if ((v = class_getInstanceVariable(c, "_cleanedUp"))) off_cleaned = ivar_getOffset(v);
	if ((v = class_getInstanceVariable(c, "_sentResponse"))) off_sent = ivar_getOffset(v);
	Method m = class_getInstanceMethod(c, sel_registerName("initWithMessage:replyPort:requestFlags:auditToken:applicationController:"));
	if (!m || off_pid < 0 || off_port < 0 || off_cleaned < 0 || off_sent < 0) { L("agent-sim springboard hooks: SpringBoard's layout is not the expected one; off\n"); return; }
	orig_init = method_setImplementation(m, (IMP)hook_init);
	dispatch_source_t t = dispatch_source_create(DISPATCH_SOURCE_TYPE_TIMER, 0, 0, dispatch_get_main_queue());
	dispatch_source_set_timer(t, dispatch_time(DISPATCH_TIME_NOW, NSEC_PER_SEC), NSEC_PER_SEC, NSEC_PER_SEC / 4);
	dispatch_source_set_event_handler_f(t, check);
	dispatch_resume(t);
	L("agent-sim springboard hooks: on\n");
}
__attribute__((constructor)) static void init(void) {
	if (strcmp(getprogname(), "SpringBoard")) return;
	const char *e = getenv("AGENT_SIM_HOOK_LOG");
	if (e && *e >= '1') {
		lg = fopen("/Volumes/SystemRoot/tmp/agent-sim-springboard.log", "a");
		verbose = *e >= '2';
	}
	_dyld_register_func_for_add_image(added);
}
