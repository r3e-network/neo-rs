import os
for k in ("PROGRAMDATA","ProgramData","APPDATA"):
    print(f"{k} = {os.environ.get(k)!r}")
