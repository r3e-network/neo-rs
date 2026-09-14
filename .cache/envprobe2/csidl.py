import ctypes
from ctypes import wintypes
shell32 = ctypes.WinDLL('shell32')
# CSIDL_COMMON_APPDATA = 0x0023, CSIDL_APPDATA = 0x001a, CSIDL_LOCAL_APPDATA = 0x001c
# use SHGetFolderPathW
for name, csidl in [('COMMON_APPDATA',0x0023),('APPDATA',0x001a),('LOCAL_APPDATA',0x001c)]:
    buf = ctypes.create_unicode_buffer(260)
    hr = shell32.SHGetFolderPathW(None, csidl, None, 0, buf)
    print(f'{name} hr=0x{hr & 0xffffffff:08x} path=[{buf.value}]')
