// The screen as an agent reads it: on SIGUSR2, a UIKit process (an app, or SpringBoard for the home screen, alerts
// and the status bar) writes its visible accessibility elements as JSON to <device data>/tmp/agent-sim/ax-<pid>.json
// (SpringBoard: ax-springboard.json).
// SpringBoard also writes front.json: the app in front and its pid. `agent-sim snapshot` sends the signals and reads
// the files.
//
// Each element: label, value, hint, identifier, traits, frame (screen points), and whether it is enabled. The walk
// follows UIKit's accessibility containers (accessibilityElements / accessibilityElementAtIndex:) and views'
// subviews, skipping hidden ones, as VoiceOver sees the screen.
//
// Loaded into every phone process by scripts/phone.sh; inert until UIKit is loaded and the signal arrives.
#include <objc/runtime.h>
#include <objc/message.h>
#include <mach-o/dyld.h>
#include <dispatch/dispatch.h>
#include <signal.h>
#include <stdbool.h>
#include <stdio.h>
#include <stdlib.h>
#include <string.h>
#include <unistd.h>
#include <sys/stat.h>

typedef struct { double x, y, w, h; } Rect;
#define SEL_(s) sel_registerName(s)
#define SEND(r, o, s, ...) ((r (*)(id, SEL, ##__VA_ARGS__))objc_msgSend)

static bool responds(id o, const char *s) { return o && SEND(bool, o, "respondsToSelector:", SEL)(o, SEL_("respondsToSelector:"), SEL_(s)); }
static id get(id o, const char *s) { return responds(o, s) ? SEND(id, o, s)(o, SEL_(s)) : nil; }
static bool getb(id o, const char *s) { return responds(o, s) ? SEND(bool, o, s)(o, SEL_(s)) : false; }
static long getl(id o, const char *s) { return responds(o, s) ? SEND(long, o, s)(o, SEL_(s)) : 0; }
static double getd(id o, const char *s) { return responds(o, s) ? SEND(double, o, s)(o, SEL_(s)) : 0; }
static const char *utf8(id s) {
	if (!s || !responds(s, "UTF8String")) return NULL;
	return SEND(const char *, s, "UTF8String")(s, SEL_("UTF8String"));
}
static bool kind(id o, const char *cls) {
	Class c = objc_getClass(cls);
	return o && c && SEND(bool, o, "isKindOfClass:", Class)(o, SEL_("isKindOfClass:"), c);
}
static unsigned long count(id a) { return a ? SEND(unsigned long, a, "count")(a, SEL_("count")) : 0; }
static id at(id a, unsigned long i) { return SEND(id, a, "objectAtIndex:", unsigned long)(a, SEL_("objectAtIndex:"), i); }
static Rect frame_of(id o) {
	if (!responds(o, "accessibilityFrame")) return (Rect){0};
	return SEND(Rect, o, "accessibilityFrame")(o, SEL_("accessibilityFrame"));
}

// a growing JSON buffer
static char *buf;
static size_t len, cap;
static void put(const char *s, size_t n) {
	if (len + n + 1 > cap) {
		cap = (len + n + 1) * 2;
		buf = realloc(buf, cap);
	}
	memcpy(buf + len, s, n);
	len += n;
	buf[len] = 0;
}
static void puts_(const char *s) { put(s, strlen(s)); }
static void putf(const char *fmt, double a, double b, double c, double d) {
	char t[160];
	int n = snprintf(t, sizeof t, fmt, a, b, c, d);
	put(t, n);
}
static void putstr(const char *s) {
	puts_("\"");
	for (const unsigned char *p = (const unsigned char *)(s ? s : ""); *p; p++) {
		char t[8];
		if (*p == '"' || *p == '\\') { t[0] = '\\'; t[1] = *p; put(t, 2); }
		else if (*p == '\n') puts_("\\n");
		else if (*p < 0x20) { snprintf(t, sizeof t, "\\u%04x", *p); puts_(t); }
		else put((const char *)p, 1);
	}
	puts_("\"");
}

// UIAccessibilityTraits bits, by name
static const struct { unsigned long bit; const char *name; } kTraits[] = {
	{1ul << 0, "button"}, {1ul << 1, "link"}, {1ul << 2, "image"}, {1ul << 3, "selected"}, {1ul << 4, "plays-sound"},
	{1ul << 5, "keyboard-key"}, {1ul << 6, "static-text"}, {1ul << 7, "summary"}, {1ul << 8, "not-enabled"},
	{1ul << 9, "updates-frequently"}, {1ul << 10, "search-field"}, {1ul << 12, "adjustable"},
	{1ul << 13, "allows-direct-interaction"}, {1ul << 14, "causes-page-turn"}, {1ul << 16, "header"},
	{1ul << 17, "tab-bar"}, {1ul << 18, "toggle"},
};

static int emitted;
static void emit(id e) {
	Rect f = frame_of(e);
	if (f.w <= 0 || f.h <= 0) return;
	const char *label = utf8(get(e, "accessibilityLabel")), *value = utf8(get(e, "accessibilityValue"));
	const char *hint = utf8(get(e, "accessibilityHint")), *ident = utf8(get(e, "accessibilityIdentifier"));
	unsigned long traits = (unsigned long)getl(e, "accessibilityTraits");
	if (kind(e, "UITextField") || kind(e, "UITextView")) {  // the text itself, and what it is
		if (!value) value = utf8(get(e, "text"));
		if (!label) label = utf8(get(e, "placeholder"));
	}
	if (!label && !value && !ident && !(traits & ((1ul << 0) | (1ul << 1) | (1ul << 10) | (1ul << 18)))) return;
	puts_(emitted++ ? ",\n{" : "\n{");
	puts_("\"label\":"); putstr(label);
	if (value) { puts_(",\"value\":"); putstr(value); }
	if (hint) { puts_(",\"hint\":"); putstr(hint); }
	if (ident) { puts_(",\"id\":"); putstr(ident); }
	puts_(",\"class\":"); putstr(object_getClassName(e));
	puts_(",\"traits\":[");
	int k = 0;
	for (size_t i = 0; i < sizeof kTraits / sizeof *kTraits; i++)
		if (traits & kTraits[i].bit) { if (k++) puts_(","); putstr(kTraits[i].name); }
	if (kind(e, "UITextField") || kind(e, "UITextView")) { if (k++) puts_(","); putstr("text-field"); }
	putf("],\"frame\":[%.1f,%.1f,%.1f,%.1f]}", f.x, f.y, f.w, f.h);
}

static void walk(id o, int depth) {
	if (!o || depth > 60 || emitted > 2000) return;
	if (getb(o, "accessibilityElementsHidden")) return;
	if (kind(o, "UIView") && (getb(o, "isHidden") || getd(o, "alpha") < 0.01)) return;
	if (getb(o, "isAccessibilityElement")) {
		emit(o);
		return;
	}
	id kids = get(o, "accessibilityElements");
	if (kids && count(kids)) {
		for (unsigned long i = 0, n = count(kids); i < n; i++) walk(at(kids, i), depth + 1);
		return;
	}
	long n = getl(o, "accessibilityElementCount");
	if (n > 0 && n < 100000) {
		for (long i = 0; i < n; i++)
			walk(SEND(id, o, "accessibilityElementAtIndex:", long)(o, SEL_("accessibilityElementAtIndex:"), i), depth + 1);
		return;
	}
	if (kind(o, "UIView")) {
		id subs = get(o, "subviews");
		for (unsigned long i = 0, m = count(subs); i < m; i++) walk(at(subs, i), depth + 1);
	}
}

static char dir[1024];
static void write_file(const char *name, const char *data, size_t n) {
	char path[1200], tmp[1240];
	snprintf(path, sizeof path, "%s/%s", dir, name);
	snprintf(tmp, sizeof tmp, "%s.tmp", path);
	FILE *f = fopen(tmp, "w");
	if (!f) return;
	fwrite(data, 1, n, f);
	fclose(f);
	rename(tmp, path);
}

// SpringBoard: which app is in front (bundle id and pid), or none (the home screen). Foreground system UI services
// (the keyboard's InputUI) are not the app.
static void write_front(void) {
	id ctl = SEND(id, (id)objc_getClass("SBApplicationController"), "sharedInstanceIfExists")(
		(id)objc_getClass("SBApplicationController"), SEL_("sharedInstanceIfExists"));
	id sb = SEND(id, (id)objc_getClass("UIApplication"), "sharedApplication")((id)objc_getClass("UIApplication"), SEL_("sharedApplication"));
	id front = get(sb, "_accessibilityFrontMostApplication");
	id apps = get(ctl, "runningApplications");
	char out[512];
	snprintf(out, sizeof out, "{\"bundle\":null,\"pid\":0}\n");
	for (unsigned long i = 0, n = front ? 1 : count(apps); i < n; i++) {
		id app = front ? front : at(apps, i), st = get(app, "processState");
		const char *b = utf8(get(app, "bundleIdentifier"));
		if (!st || !getb(st, "isForeground") || !b || strstr(b, "InputUI")) continue;
		snprintf(out, sizeof out, "{\"bundle\":\"%s\",\"pid\":%ld}\n", b, getl(st, "pid"));
		break;
	}
	write_file("front.json", out, strlen(out));
}

// SpringBoard: requests agent-sim leaves in cmd.txt before the signal ("pasteboard-get", or "pasteboard-set" and the
// text on the following lines); the answer goes to pasteboard.txt. SpringBoard may read the pasteboard at any time;
// a background process may not.
static void pasteboard_request(void) {
	char path[1200];
	snprintf(path, sizeof path, "%s/cmd.txt", dir);
	FILE *f = fopen(path, "r");
	if (!f) return;
	static char req[65536];
	size_t n = fread(req, 1, sizeof req - 1, f);
	fclose(f);
	unlink(path);
	req[n] = 0;
	id pb = SEND(id, (id)objc_getClass("UIPasteboard"), "generalPasteboard")((id)objc_getClass("UIPasteboard"), SEL_("generalPasteboard"));
	if (!pb) return;
	if (!strncmp(req, "pasteboard-set\n", 15)) {
		id str = SEND(id, (id)objc_getClass("NSString"), "stringWithUTF8String:", const char *)(
			(id)objc_getClass("NSString"), SEL_("stringWithUTF8String:"), req + 15);
		SEND(void, pb, "setString:", id)(pb, SEL_("setString:"), str);
		write_file("pasteboard.txt", "ok", 2);
	} else if (!strncmp(req, "pasteboard-get", 14)) {
		const char *t = utf8(get(pb, "string"));
		write_file("pasteboard.txt", t ? t : "", t ? strlen(t) : 0);
	}
}

static void dump(void *ctx) {
	if (!strcmp(getprogname(), "SpringBoard")) pasteboard_request();
	len = 0;
	emitted = 0;
	id app = SEND(id, (id)objc_getClass("UIApplication"), "sharedApplication")((id)objc_getClass("UIApplication"), SEL_("sharedApplication"));
	char head[256];
	snprintf(head, sizeof head, "{\"pid\":%d,\"process\":\"%s\",\"windows\":[", getpid(), getprogname());
	puts_(head);
	id wins = get(app, "windows");
	int w = 0;
	for (unsigned long i = 0, n = count(wins); i < n; i++) {
		id win = at(wins, i);
		if (getb(win, "isHidden") || getd(win, "alpha") < 0.01) continue;
		if (w++) puts_(",");
		puts_("{\"class\":"); putstr(object_getClassName(win));
		putf(",\"level\":%.0f,\"elements\":[", getd(win, "windowLevel"), 0, 0, 0);
		emitted = 0;
		walk(win, 0);
		puts_("]}");
	}
	puts_("]}\n");
	char name[64];
	bool sb = !strcmp(getprogname(), "SpringBoard");
	if (sb) snprintf(name, sizeof name, "ax-springboard.json");
	else snprintf(name, sizeof name, "ax-%d.json", getpid());
	write_file(name, buf, len);
	if (sb) write_front();
}

static void start(void) {
	const char *h = getenv("CFFIXED_USER_HOME");
	if (!h || !*h) h = getenv("HOME");
	if (!h) return;
	snprintf(dir, sizeof dir, "%s", h);
	char *c = strstr(dir, "/Containers/");  // an app's home is its container below the device's data directory
	if (c) *c = 0;
	strncat(dir, "/tmp/agent-sim", sizeof dir - strlen(dir) - 1);
	mkdir(dir, 0755);
	signal(SIGUSR2, SIG_IGN);
	dispatch_source_t s = dispatch_source_create(DISPATCH_SOURCE_TYPE_SIGNAL, SIGUSR2, 0, dispatch_get_main_queue());
	dispatch_source_set_event_handler_f(s, dump);
	dispatch_resume(s);
}
static void added(const struct mach_header *mh, intptr_t slide) {
	static bool on;
	if (!on && objc_getClass("UIApplication")) {
		on = true;
		start();
	}
}
__attribute__((constructor)) static void init(void) { _dyld_register_func_for_add_image(added); }
