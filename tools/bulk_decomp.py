"""Read-only bulk decompiler: Ghidra pseudo-C for many acs.exe functions at once.

Works on a COPY of the Ghidra project (the real one stays untouched and may be open in the
kawaiidra MCP at the same time):

    mkdir re/ghidra_copy/acs
    cp    ghidra/acs/acs.gpr  re/ghidra_copy/acs/
    cp -r ghidra/acs/acs.rep  re/ghidra_copy/acs/        (no lock files)

Needs a Python with JPype1, GHIDRA_INSTALL_DIR and a JDK (JAVA_HOME), and re/index/functions.tsv
from tools/pdb_index.py.

Usage:
  bulk_decomp.py [--project re/ghidra_copy/acs] [--out re/decomp] [--threads 10]
                 [--libs a.lib,b.lib | --all-kunos] [--match REGEX] [--force]

Output: <out>/<obj stem>/<Class>__<method>__<va>.c   (one file per function, with a header
giving address, size, object file and direct callers), plus <out>/INDEX.tsv.
"""
import argparse, glob, os, re, sys, threading, time, queue

HERE = os.path.dirname(os.path.abspath(__file__))
ROOT = os.path.dirname(HERE)

# third-party / runtime code that is not worth decompiling
SKIP_LIBS = {"ode_single.lib", "ppfx_dx11_x64.lib", "EndorFanatecSdk64_VS2013_MD.lib", "FW1FontWrapper.lib",
             "libovr.lib", "MSVCRT.lib", "msvcprt.lib", "Delayimp.lib"}
SKIP_NAME = re.compile(r"^(std::|`|_?_?(scrt|security|crt|CRT)|operator (new|delete)|atexit|_RTC|__GS|_Init_thread)")


def qualified(und):
    """'public: void __cdecl Car::step(float)' -> 'Car::step'"""
    s = und
    depth, cut = 0, len(s)
    # strip the argument list: last top-level '('
    for i in range(len(s) - 1, -1, -1):
        c = s[i]
        if c == ")":
            depth += 1
        elif c == "(":
            depth -= 1
            if depth == 0:
                cut = i
                break
    s = s[:cut].strip()
    # the qualified name is the last token that is not inside <>
    depth, start = 0, 0
    for i, c in enumerate(s):
        if c == "<":
            depth += 1
        elif c == ">":
            depth -= 1
        elif c == " " and depth == 0:
            start = i + 1
    s = s[start:]
    return s.lstrip("*&") or und


def fname(q, va):
    q = q.replace("operator()", "operator_call").replace("operator=", "operator_assign")
    q = re.sub(r"<lambda_([0-9a-f]{6})[0-9a-f]*>", r"lambda_\1", q)
    q = q.replace("::~", "::dtor_").replace("::", "__")
    q = re.sub(r"[^A-Za-z0-9_]+", "_", q).strip("_")
    return "%s__%x.c" % (q[:110], va)


def load_index():
    rows = []
    with open(os.path.join(ROOT, "re", "index", "functions.tsv"), encoding="utf-8") as f:
        next(f)
        for l in f:
            va, size, lib, obj, name = l.rstrip("\n").split("\t")
            rows.append((int(va, 16), int(size), lib, obj, name))
    return rows


def load_callers():
    callers = {}
    p = os.path.join(ROOT, "re", "index", "calls.tsv")
    if not os.path.exists(p):
        return callers
    with open(p, encoding="utf-8") as f:
        next(f)
        for l in f:
            cva, cname, kind, tva, tname = l.rstrip("\n").split("\t")
            if kind in ("call", "jmp", "ptr"):
                callers.setdefault(int(tva, 16), []).append((kind, cva, qualified(cname)))
    return callers


def start_jvm():
    import jpype, jpype.imports
    gh = os.environ.get("GHIDRA_INSTALL_DIR", r"C:\ghidra_12.1.2_PUBLIC")
    jars = []
    for sub in ("Framework", "Features", "Processors/x86"):
        pat = os.path.join(gh, "Ghidra", sub, "**", "lib", "*.jar")
        jars += glob.glob(pat, recursive=True)
    jars = [j for j in jars if "PyGhidra" not in j and "Jython" not in j]
    jpype.startJVM("-Xmx8G", "-Dghidra.root=" + gh, "-Djava.awt.headless=true",
                   classpath=os.pathsep.join(jars), convertStrings=True)
    from ghidra.framework import Application, HeadlessGhidraApplicationConfiguration
    from ghidra import GhidraApplicationLayout
    if not Application.isInitialized():
        Application.initializeApplication(GhidraApplicationLayout(), HeadlessGhidraApplicationConfiguration())


def main():
    ap = argparse.ArgumentParser()
    ap.add_argument("--project", default=os.path.join(ROOT, "re", "ghidra_copy", "acs"))
    ap.add_argument("--out", default=os.path.join(ROOT, "re", "decomp"))
    ap.add_argument("--threads", type=int, default=10)
    ap.add_argument("--libs", default="")
    ap.add_argument("--all-kunos", action="store_true")
    ap.add_argument("--match", default="")
    ap.add_argument("--force", action="store_true")
    ap.add_argument("--timeout", type=int, default=120)
    a = ap.parse_args()

    libs = set(x for x in a.libs.split(",") if x)
    rx = re.compile(a.match) if a.match else None
    todo = []
    for va, size, lib, obj, name in load_index():
        q = qualified(name)
        if libs:
            if lib not in libs:
                continue
        elif lib in SKIP_LIBS or lib == "-" or not a.all_kunos and not rx:
            continue
        if rx and not rx.search(q):
            continue
        if not rx and SKIP_NAME.search(q):
            continue
        stem = os.path.splitext(obj)[0]
        path = os.path.join(a.out, stem, fname(q, va))
        if os.path.exists(path) and not a.force:
            continue
        todo.append((va, size, lib, obj, name, q, path))
    print("functions to decompile:", len(todo), flush=True)
    if not todo:
        return
    callers = load_callers()

    start_jvm()
    from ghidra.base.project import GhidraProject
    from ghidra.app.decompiler import DecompInterface, DecompileOptions
    from ghidra.util.task import ConsoleTaskMonitor
    project = GhidraProject.openProject(os.path.abspath(a.project), "acs", True)
    program = project.openProgram("/", "acs.exe", True)
    space = program.getAddressFactory().getDefaultAddressSpace()
    fm = program.getFunctionManager()
    print("program open:", program.getName(), flush=True)

    q = queue.Queue()
    for t in todo:
        q.put(t)
    done, failed = [], []
    lock = threading.Lock()

    def worker():
        di = DecompInterface()
        opts = DecompileOptions()
        di.setOptions(opts)
        di.openProgram(program)
        mon = ConsoleTaskMonitor()
        while True:
            try:
                va, size, lib, obj, name, qn, path = q.get_nowait()
            except queue.Empty:
                break
            try:
                fn = fm.getFunctionAt(space.getAddress(va))
                if fn is None:
                    with lock:
                        failed.append((va, qn, "no function at address in the Ghidra database"))
                    continue
                res = di.decompileFunction(fn, a.timeout, mon)
                df = res.getDecompiledFunction() if res is not None else None
                if df is None:
                    with lock:
                        failed.append((va, qn, str(res.getErrorMessage()) if res is not None else "no result"))
                    continue
                code = str(df.getC()).replace("\r\n", "\n")
                h = ["/*", " * Function : " + qn, " * Symbol   : " + name, " * Address  : 0x%x" % va,
                     " * Size     : %d bytes" % size, " * Object   : %s / %s" % (lib, obj), " * Callers  :"]
                cs = callers.get(va, [])
                h += [" *   %s  %s @ 0x%s" % (k, n, c) for k, c, n in cs[:40]] or [" *   (no direct call sites; virtual or unused)"]
                if len(cs) > 40:
                    h.append(" *   ... %d more" % (len(cs) - 40))
                h += [" * Source   : Ghidra 12.1.2 pseudo-C of acs.exe with acs.pdb loaded (tools/bulk_decomp.py).",
                      " *            Body below is unmodified decompiler output.", " */", ""]
                os.makedirs(os.path.dirname(path), exist_ok=True)
                with open(path, "w", encoding="utf-8", newline="\n") as f:
                    f.write("\n".join(h) + code.lstrip("\n"))
                with lock:
                    done.append((va, size, lib, obj, qn, os.path.relpath(path, a.out).replace("\\", "/")))
                    if len(done) % 500 == 0:
                        print("  %d / %d  (%.0fs)" % (len(done), len(todo), time.time() - t0), flush=True)
            except Exception as e:  # keep going
                with lock:
                    failed.append((va, qn, repr(e)[:200]))
        di.dispose()

    t0 = time.time()
    ths = [threading.Thread(target=worker) for _ in range(a.threads)]
    for t in ths:
        t.start()
    for t in ths:
        t.join()
    idx = os.path.join(a.out, "INDEX.tsv")
    old = {}
    if os.path.exists(idx):
        with open(idx, encoding="utf-8") as f:
            next(f)
            for l in f:
                p = l.rstrip("\n").split("\t")
                old[p[0]] = p
    for va, size, lib, obj, qn, rel in done:
        old["%x" % va] = ["%x" % va, str(size), lib, obj, qn, rel]
    with open(idx, "w", encoding="utf-8", newline="\n") as f:
        f.write("va\tsize\tlib\tobj\tname\tfile\n")
        for k in sorted(old):
            f.write("\t".join(old[k]) + "\n")
    with open(os.path.join(a.out, "FAILED.tsv"), "a", encoding="utf-8", newline="\n") as f:
        for va, qn, why in failed:
            f.write("%x\t%s\t%s\n" % (va, qn, why))
    print("done %d, failed %d, %.0fs" % (len(done), len(failed), time.time() - t0), flush=True)
    program.release(project) if hasattr(program, "release") else None
    os._exit(0)


if __name__ == "__main__":
    main()
