"""Native verification against the app's own main window; restores the pointer afterwards."""
import ctypes as c
from ctypes import wintypes as w
import json,time
from pathlib import Path
from PIL import ImageGrab

u=c.WinDLL('user32',use_last_error=True)
u.SetProcessDpiAwarenessContext.argtypes=[c.c_void_p]
u.SetProcessDpiAwarenessContext(c.c_void_p(-4))
u.FindWindowW.argtypes=[w.LPCWSTR,w.LPCWSTR];u.FindWindowW.restype=w.HWND
u.SendMessageW.argtypes=[w.HWND,w.UINT,w.WPARAM,w.LPARAM];u.SendMessageW.restype=w.LPARAM
u.GetForegroundWindow.restype=w.HWND
u.SetForegroundWindow.argtypes=[w.HWND]
hwnd=u.FindWindowW(None,'Android DevKit')
assert hwnd,'App main HWND'
root=Path(__file__).resolve().parent.parent
dom=json.loads((root/'.qa/app/window-dom.json').read_text())
origin=w.POINT(0,0);u.ClientToScreen(hwnd,c.byref(origin))
r=dom['rect'];scale=dom['scale']
x=origin.x+int((r['x']+r['width']/2)*scale)
y=origin.y+int((r['y']+r['height']/2)*scale)
hit=u.SendMessageW(hwnd,0x84,0,((y&0xffff)<<16)|(x&0xffff))
assert hit==9,f'Maximize area must return HTMAXBUTTON, got {hit}'
cursor=w.POINT();u.GetCursorPos(c.byref(cursor));foreground=u.GetForegroundWindow()
try:
 u.SetForegroundWindow(hwnd);u.SetCursorPos(x,y);time.sleep(1.2)
 image=ImageGrab.grab(bbox=(max(0,x-510),max(0,y-35),x+95,y+410))
 image.save(root/'.qa/app/native-snap-hover.png')
finally:
 u.SetCursorPos(cursor.x,cursor.y)
 if foreground:u.SetForegroundWindow(foreground)
report={'maximizeHitTest':hit,'dpiScale':scale,'hwnd':int(hwnd),'pointerRestored':True}
(root/'.qa/app/window-report.json').write_text(json.dumps(report,indent=2))
print(json.dumps(report,indent=2))
