import importlib.util, sys

spec = importlib.util.spec_from_file_location("mfv", "scripts/mainnet-full-verify.py")
mfv = importlib.util.module_from_spec(spec)
spec.loader.exec_module(mfv)

c = mfv.PooledRpcClient("http://127.0.0.1:10332", timeout=15)
print("getblockcount:", c.call("getblockcount", []))
for h in [0, 1, 2, 100, 1000, 10000, 20000, 21370, 21371, 21372]:
    try:
        r = c.call("getstateroot", [h])
        print(f"h={h:<6} root={(r or {}).get('roothash') or (r or {}).get('rootHash')}")
    except Exception as e:
        print(f"h={h:<6} ERR {e}")
c.close()
