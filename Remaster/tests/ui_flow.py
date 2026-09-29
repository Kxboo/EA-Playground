"""Exercise real Tk controls, workers, previews and exports on local assets."""
import sys
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import json
import time
import ctypes
from ctypes import wintypes
import struct
import core
from archives import BigArchive
from app import App, messagebox


def capture(app,path):
    # Capture this application's window only, including its native Tk rendering.
    u,g=ctypes.windll.user32,ctypes.windll.gdi32
    u.GetParent.argtypes=[wintypes.HWND]; u.GetParent.restype=wintypes.HWND
    u.GetWindowDC.argtypes=[wintypes.HWND]; u.GetWindowDC.restype=wintypes.HDC
    g.CreateCompatibleDC.argtypes=[wintypes.HDC];g.CreateCompatibleDC.restype=wintypes.HDC
    g.CreateCompatibleBitmap.argtypes=[wintypes.HDC,ctypes.c_int,ctypes.c_int];g.CreateCompatibleBitmap.restype=wintypes.HBITMAP
    g.SelectObject.argtypes=[wintypes.HDC,wintypes.HANDLE];g.SelectObject.restype=wintypes.HANDLE
    g.GetDIBits.argtypes=[wintypes.HDC,wintypes.HBITMAP,wintypes.UINT,wintypes.UINT,ctypes.c_void_p,ctypes.c_void_p,wintypes.UINT]
    g.DeleteDC.argtypes=[wintypes.HDC];g.DeleteObject.argtypes=[wintypes.HANDLE]
    u.ReleaseDC.argtypes=[wintypes.HWND,wintypes.HDC]
    u.PrintWindow.argtypes=[wintypes.HWND,wintypes.HDC,wintypes.UINT]
    hwnd=u.GetParent(app.winfo_id())
    rect=wintypes.RECT()
    u.GetWindowRect(hwnd,ctypes.byref(rect))
    w,h=rect.right-rect.left,rect.bottom-rect.top
    dc=u.GetWindowDC(hwnd);mem=g.CreateCompatibleDC(dc)
    bitmap=g.CreateCompatibleBitmap(dc,w,h);old=g.SelectObject(mem,bitmap)
    try:
        if not u.PrintWindow(hwnd,mem,2):raise RuntimeError('PrintWindow failed')
        buf=ctypes.create_string_buffer(w*h*4)
        info=ctypes.create_string_buffer(struct.pack('<IiiHHIIiiII',40,w,-h,1,32,0,w*h*4,0,0,0,0))
        if not g.GetDIBits(mem,bitmap,0,h,buf,info,0):raise RuntimeError('GetDIBits failed')
        rgba=bytearray(buf.raw)
        rgba[0::4],rgba[2::4]=rgba[2::4],rgba[0::4]
        rgba[3::4]=b'\xff'*(w*h)
        path.write_bytes(core.encode(rgba,w,h))
    finally:
        g.SelectObject(mem,old);g.DeleteObject(bitmap);g.DeleteDC(mem);u.ReleaseDC(hwnd,dc)


def main():
    errors=[]
    messagebox.showerror=lambda title,text:errors.append(title+': '+text)
    app=App()
    app.output=core.HOME/'exports/ui_test'
    def settle():
        deadline=time.time()+90
        idle=0
        while time.time()<deadline:
            app.update()
            if not app.busy:
                idle+=1
                if idle>15:break
            else:idle=0
            time.sleep(.015)
        assert not app.busy,'UI job timed out'
        assert not errors,errors
    settle()
    # A real archive -> sibling extraction -> actual model preview.
    archives=[]
    for p in list(core.DEFAULT_DATA.rglob('*.big'))+list(core.DEFAULT_DATA.rglob('*.viv')):
        if any(Path(e.name).name=='basketball.o' for e in BigArchive(p).entries):archives.append(p)
    assert archives,'Basketball source archive not found'
    app.open_path(min(archives,key=lambda p:p.stat().st_size));settle()
    candidates=[e for e in app.asset.entries if Path(e.name).name=='basketball.o']
    assert candidates
    app.rows.selection_set(str(candidates[0].index));app.open_entry();settle()
    assert app.kind=='model'
    app.open_path(core.DEFAULT_OLD/'WorldProps/basketball.o');settle()
    app.export_model('glb');settle()
    capture(app,core.HOME/'docs/ui-model.png')
    app.open_path(core.DEFAULT_OLD/'WorldProps/basketball.gsh');settle()
    assert app.preview.photo
    app.export_textures(True);settle()
    capture(app,core.HOME/'docs/ui-texture.png')
    app.open_path(core.HOME/'reference/player_anims.anm');settle()
    assert app.skeleton and len(app.asset.blocks)==265
    app.rows.selection_set('0');settle()
    assert app.clip and app.clip.sample_count>0
    app.play()
    for _ in range(20):app.update();time.sleep(.03)
    app.playing=False
    assert app.frame>0
    app.export_animations(False);settle()
    capture(app,core.HOME/'docs/ui-animation.png')
    app.open_path(core.HOME/'reference/player_skel.ske');settle()
    app.export_skeleton();settle()
    app.open_path(next(core.DEFAULT_DATA.rglob('home.csv')));settle()
    assert app.kind=='inspection' and app.asset['rows']
    app.inspect_format();settle()
    assert 'Delimited table' in app.details.get('1.0','end')
    app.open_path(core.DEFAULT_OLD/'placeables/rc_trackcar/rc_track_car.o');settle()
    assert app.kind=='inspection' and app.asset['payload']['status']=='unsupported_or_invalid'
    (core.HOME/'docs/ui-verification.json').write_text(json.dumps({'status':'ok','archive_open':bool(archives),'model_glb':True,'texture_png':True,'animation_playback_and_export':True,'skeleton_export':True,'csv_inspection':True,'unsupported_model_inspection':True,'errors':errors},indent=2))
    app.destroy()
    print('UI flow passed')


if __name__=='__main__':main()
