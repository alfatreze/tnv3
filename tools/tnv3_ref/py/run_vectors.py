import hashlib
import os
import sys

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from tnv3_demux import demux, TnvError

vd = os.path.join(os.path.dirname(os.path.abspath(__file__)), "..", "vectors")


def md5(b):
    return hashlib.md5(b).hexdigest()


fails = 0
total = 0
for line in open(os.path.join(vd, "expected.txt")):
    line = line.strip()
    if not line:
        continue
    note = ""
    if "#" in line:
        line, note = line.split("#", 1)
    parts = line.split()
    name, kind = parts[0], parts[1]
    kv = dict(p.split("=", 1) for p in parts[2:])
    total += 1
    try:
        out = demux(os.path.join(vd, name))
        err = None
    except TnvError as e:
        out, err = None, str(e)
    except Exception as e:
        out, err = None, "UNEXPECTED %s: %s" % (type(e).__name__, e)
    if kind == "valid":
        if out is None:
            ok = False
            msg = "rejected: " + err
        else:
            got = dict(tvc=md5(out[0]),
                       tac=md5(out[1]) if out[1] is not None else None,
                       subs=md5(out[2]) if out[2] is not None else None)
            ok = True
            msg = ""
            for k in ("tvc", "tac", "subs"):
                if got[k] != kv.get(k):
                    ok = False
                    msg += " %s got %s want %s" % (k, got[k], kv.get(k))
    else:
        ok = out is None and not (err or "").startswith("UNEXPECTED")
        msg = "mine: %s | ref:%s" % (err, note)
    print("PASS" if ok else "FAIL", name, msg)
    fails += not ok
print("%d/%d passed" % (total - fails, total))
sys.exit(1 if fails else 0)
