#!/usr/bin/env python3
"""Run the WebGL2 probe inside WebKitGTK and print what it found.

Spike C's capability clause is engine-specific: §2.3 asks about "this machine's
WebKitGTK", because that is the webview Tauri uses on Linux. Chrome would answer a
different question. So this serves the probe over loopback, opens it in a WebKitGTK
webview, and waits for the page to POST its report back.

**Two bindings, and the difference is the point of `--engine`.** Spike C ran in
Epiphany, which is **webkitgtk-6.0** (GTK 4). Tauri v2 on Linux embeds
**webkit2gtk-4.1** (GTK 3). Both are the same upstream WebKit — 2.52.5 here — and
`SPIKE-C.md` recorded that the capability result had not been confirmed in the one
the product will actually ship inside, leaving it to the first v0.1 build. It does
not have to wait that long: the 4.1 path below embeds a WebView directly, so the same
probe answers for both and the two reports can be read side by side.

Usage:  python3 run-probe.py [--engine epiphany|webkit2gtk-4.1] [--keep-open]
"""
import argparse
import http.server
import json
import os
import shutil
import socketserver
import subprocess
import sys
import tempfile
import threading
import time

HERE = os.path.dirname(os.path.abspath(__file__))
RESULT = {}
DONE = threading.Event()
VERBOSE = False


class Handler(http.server.SimpleHTTPRequestHandler):
    def __init__(self, *a, **kw):
        super().__init__(*a, directory=HERE, **kw)

    def do_POST(self):
        if self.path != "/result":
            self.send_error(404)
            return
        n = int(self.headers.get("content-length", 0))
        try:
            RESULT.update(json.loads(self.rfile.read(n) or b"{}"))
        except json.JSONDecodeError as e:
            RESULT["parse_error"] = str(e)
        self.send_response(204)
        self.end_headers()
        DONE.set()

    def log_message(self, fmt, *a):
        if VERBOSE:
            sys.stderr.write("  http: " + (fmt % a) + "\n")


def report(r, engine="?"):
    if not r:
        print("no result received")
        return 1

    caps, limits = r.get("caps", {}), r.get("limits", {})
    print("=" * 72)
    print(f"Spike C — WebGL2 probe, WebKitGTK via {engine}")
    print("=" * 72)
    print(f"  user agent   {r.get('ua','?')}")
    print(f"  GL version   {caps.get('version','?')}")
    print(f"  GLSL         {caps.get('glsl','?')}")
    print(f"  renderer     {caps.get('renderer','?')}")
    if caps.get("unmaskedRenderer"):
        print(f"  unmasked     {caps['unmaskedRenderer']}")

    print("\n§2.3's clause — the working space has to be representable:")
    v = caps.get("EXT_color_buffer_float")
    print(f"  {'OK ' if v else 'FAIL'} EXT_color_buffer_float: {v}")
    st = caps.get("framebufferStatus")
    print(f"  {'OK ' if caps.get('rgba16fRenderable') else 'FAIL'} RGBA16F colour attachment: {st}")
    lin = caps.get("rgba16fLinearFilters")
    print(f"  {'OK ' if lin else 'FAIL'} RGBA16F linear filtering, measured: "
          f"sampled {caps.get('rgba16fLinearSample')} at the midpoint "
          f"(0.5 = LINEAR, 0 or 1 = NEAREST)")
    print(f"  --  OES_texture_half_float_linear: {caps.get('OES_texture_half_float_linear')} "
          f"(absent by design in WebGL2 — RGBA16F filtering is core, so the string "
          f"proves nothing; the measurement above is the answer)")

    print("\nTranspiled shader:")
    sb = r.get("shaderBytes", {})
    print(f"  {'OK ' if caps.get('transpiledShaderCompiles') else 'FAIL'} "
          f"naga-generated GLSL compiled and linked "
          f"(vert {sb.get('vert','?')} B, frag {sb.get('frag','?')} B)")
    ubo = r.get("ubo", {})
    print(f"      uniform block {ubo.get('name','?')} = {ubo.get('size','?')} bytes "
          f"of {limits.get('maxUniformBlockSize','?')} available")
    print(f"      offsets resolved: {', '.join(r.get('uboOffsets', {}).keys()) or 'NONE'}")

    perf = r.get("perf", [])
    if perf:
        proxy = r.get("proxy", {})
        print(f"\n§7.3 budget — 16 ms per frame at proxy "
              f"({proxy.get('w')}x{proxy.get('h')}, {proxy.get('megapixels')} MP)")
        print(f"  clock resolution {r.get('timerResolutionMs','?')} ms, "
              f"amortised over batches of {perf[0].get('batch','?')} frames:")
        print(f"  {'layers':>7}  {'median':>8}  {'p95':>8}  {'min':>7}  {'max':>7}   verdict")
        for m in perf:
            ok = "within budget" if m["p95"] <= 16.0 else "OVER BUDGET"
            star = " <- §7.3 bound" if m["layers"] == 6 else ""
            print(f"  {m['layers']:>7}  {m['median']:>7.2f}ms {m['p95']:>7.2f}ms "
                  f"{m['min']:>6.2f}ms {m['max']:>6.2f}ms   {ok}{star}")

    ag = r.get("agreement")
    if ag:
        print("\n§0's invariant — one shader source, preview and export:")
        if ag.get("skipped"):
            print(f"  --  skipped: {ag['skipped']}")
        elif ag.get("invalid"):
            print(f"  FAIL {ag['invalid']}")
            print(f"      missing: {', '.join(ag.get('missingFields', []))}")
        elif ag.get("error"):
            print(f"  FAIL {ag['error']}")
        else:
            ok = ag["max"] <= 0.01
            print(f"  {'OK ' if ok else 'FAIL'} WebGL2 (naga GLSL) vs wgpu (WGSL), same source, "
                  f"{ag['samples']} channel samples")
            print(f"      max |diff| {ag['max']}   mean {ag['mean']}   "
                  f"over 1/255: {ag['overOneCode']}")
            print(f"      orientation: {ag['orientation']} "
                  f"(direct {ag['directMax']} / flipped {ag['flippedMax']})")

    print(f"\n  gl error at end: {caps.get('finalGlError','?')}")
    if r.get("errors"):
        print("\nERRORS:")
        for e in r["errors"]:
            print("  " + e.replace("\n", "\n  "))
    print("=" * 72)
    return 0 if r.get("ok") else 1


def run_in_epiphany(url, timeout, keep_open):
    """The original path: Epiphany 50, which is webkitgtk-6.0 on GTK 4."""
    profile = tempfile.mkdtemp(prefix="photodesk-spike-c-")
    proc = subprocess.Popen(
        # --profile already implies a private instance. Passing both is refused
        # ("Cannot use --private-instance and --profile at the same time")
        # and epiphany exits before loading anything, which looks exactly
        # like a page that ran and never reported.
        ["epiphany", "--profile", profile, url],
        stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL,
    )
    try:
        if not DONE.wait(timeout):
            print(f"timed out after {timeout}s with no report", file=sys.stderr)
    finally:
        if not keep_open:
            time.sleep(0.4)
            proc.terminate()
            try:
                proc.wait(timeout=5)
            except subprocess.TimeoutExpired:
                proc.kill()
        shutil.rmtree(profile, ignore_errors=True)


def run_in_webkit2gtk_41(url, timeout, keep_open):
    """The webview Tauri v2 actually embeds on Linux: webkit2gtk-4.1 on GTK 3.

    Driven directly rather than through a browser, because there is no browser that
    ships the 4.1 binding — Epiphany moved to 6.0 — and "the engine is the same
    version" is a claim about the binding as much as about WebKit. A real window is
    shown rather than an offscreen surface: WebGL wants a GL context, and a probe that
    silently fell back to software would answer a question nobody asked.
    """
    import gi

    gi.require_version("Gtk", "3.0")
    gi.require_version("WebKit2", "4.1")
    from gi.repository import GLib, Gtk, WebKit2

    print(
        f"engine: webkit2gtk-4.1, WebKit "
        f"{WebKit2.get_major_version()}.{WebKit2.get_minor_version()}."
        f"{WebKit2.get_micro_version()}",
        file=sys.stderr,
    )

    view = WebKit2.WebView.new_with_context(WebKit2.WebContext.new_ephemeral())
    settings = view.get_settings()
    settings.set_enable_webgl(True)
    settings.set_enable_write_console_messages_to_stdout(VERBOSE)
    view.set_settings(settings)

    window = Gtk.Window(title="PhotoDesk — Spike C probe (webkit2gtk-4.1)")
    window.set_default_size(900, 700)
    window.add(view)
    window.connect("destroy", Gtk.main_quit)
    window.show_all()
    view.load_uri(url)

    deadline = time.monotonic() + timeout

    def tick():
        if DONE.is_set():
            if keep_open:
                return True
            Gtk.main_quit()
            return False
        if time.monotonic() > deadline:
            print(f"timed out after {timeout}s with no report", file=sys.stderr)
            Gtk.main_quit()
            return False
        return True

    GLib.timeout_add(200, tick)
    Gtk.main()


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument(
        "--engine",
        choices=("epiphany", "webkit2gtk-4.1"),
        default="epiphany",
        help="epiphany is webkitgtk-6.0 (what Spike C measured); "
             "webkit2gtk-4.1 is what Tauri v2 embeds",
    )
    ap.add_argument("--timeout", type=float, default=90.0)
    ap.add_argument("--keep-open", action="store_true")
    ap.add_argument("--json", help="also write the raw report here")
    ap.add_argument("--verbose", action="store_true", help="log HTTP requests")
    args = ap.parse_args()
    global VERBOSE
    VERBOSE = args.verbose

    if not os.path.exists(os.path.join(HERE, "generated", "adjust.frag")):
        sys.exit("generated/adjust.frag missing — run: cargo test -p photodesk-renderer-spike")

    # Threading is not optional: the page fetches both generated shaders with
    # Promise.all, and a single-threaded server serialises them into a stall that
    # looks exactly like a page which never ran.
    class Server(socketserver.ThreadingMixIn, http.server.HTTPServer):
        daemon_threads = True
        allow_reuse_address = True

    with Server(("127.0.0.1", 0), Handler) as srv:
        port = srv.server_address[1]
        threading.Thread(target=srv.serve_forever, daemon=True).start()
        url = f"http://127.0.0.1:{port}/probe.html"
        print(f"serving {HERE} at {url}", file=sys.stderr)

        if args.engine == "epiphany":
            run_in_epiphany(url, args.timeout, args.keep_open)
        else:
            run_in_webkit2gtk_41(url, args.timeout, args.keep_open)

    if args.json and RESULT:
        with open(args.json, "w") as f:
            json.dump(RESULT, f, indent=2)
        print(f"raw report written to {args.json}", file=sys.stderr)
    return report(RESULT, args.engine)


if __name__ == "__main__":
    sys.exit(main())
