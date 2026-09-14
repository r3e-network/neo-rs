import gzip, json, urllib.request, sys

def call(url, method, params):
    body = json.dumps({"jsonrpc":"2.0","id":1,"method":method,"params":params}).encode()
    req = urllib.request.Request(url, data=body, headers={"Content-Type":"application/json","Accept-Encoding":"identity"})
    raw = urllib.request.urlopen(req, timeout=10).read()
    if raw.startswith(b"\x1f\x8b"):
        raw = gzip.decompress(raw)
    return json.loads(raw.decode("utf-8"))

LOCAL = "http://127.0.0.1:10332"
try:
    print("getblockcount =", call(LOCAL, "getblockcount", []).get("result"))
except Exception as e:
    print("getblockcount ERR", e)

for h in [0, 1, 2, 3, 10, 50, 100, 1000, 5000, 10000, 15000, 20000, 21000, 21372, 21373]:
    try:
        r = call(LOCAL, "getstateroot", [h])
        if "error" in r and r["error"]:
            print(f"h={h:<6} ERROR {r['error'].get('code')} {r['error'].get('message')}")
        else:
            res = r.get("result") or {}
            print(f"h={h:<6} root={res.get('roothash') or res.get('rootHash')}")
    except Exception as e:
        print(f"h={h:<6} EXC {e}")
