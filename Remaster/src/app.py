"""EAGL Remaster desktop workbench. Tk runs only on the main thread."""
import argparse
import json
import os
from pathlib import Path
import queue
import subprocess
import sys
import threading
import time
import traceback
import tkinter as tk
from tkinter import ttk, filedialog, messagebox
import core
import research
from core import HOME, SOURCE, DEFAULT_DATA, DEFAULT_OLD
from archives import BigArchive
from preview import Preview


class App(tk.Tk):
    def __init__(self):
        super().__init__()
        self.title('EAGL Remaster — EA Playground')
        self.geometry('1240x830')
        self.minsize(940,650)
        self.option_add('*Font', ('Segoe UI',10))
        ttk.Style(self).theme_use('clam')
        self.events = queue.Queue()
        self.busy = False
        self.cancel = threading.Event()
        self.files = []
        self.asset = None
        self.path = None
        self.kind = None
        self.skeleton = None
        self.clip = None
        self.frame = 0
        self.playing = False
        self.extra_textures = []
        self.settings = self.read_settings()
        self.root_path = Path(self.settings.get('data_root',str(DEFAULT_DATA)))
        self.output = Path(self.settings.get('output',str(HOME/'exports')))
        self.build_ui()
        self.protocol('WM_DELETE_WINDOW',self.close)
        self.after(60,self.poll)
        self.after(100,self.scan)

    def read_settings(self):
        try:
            return json.loads((HOME/'settings.json').read_text('utf-8'))
        except (OSError,ValueError):
            return {}

    def build_ui(self):
        toolbar = ttk.Frame(self,padding=10)
        toolbar.pack(fill='x')
        for text,cmd in [('Open file…',self.open_dialog),('DATA',lambda:self.set_root(DEFAULT_DATA)),('Old attempts',lambda:self.set_root(DEFAULT_OLD)),('Reference animations',lambda:self.set_root(HOME/'reference')),('Folder…',self.choose_root),('Exports',lambda:os.startfile(self.output)),('Output folder…',self.choose_output)]:
            ttk.Button(toolbar,text=text,command=cmd).pack(side='left',padx=(0,5))
        menu = tk.Menu(self)
        tools = tk.Menu(menu,tearoff=False)
        tools.add_command(label='Open existing extraction tools folder',command=lambda:os.startfile(SOURCE/'extraction tools'))
        tools.add_command(label='Launch SSX MultiTool',command=lambda:self.launch_tool('SSX.Multitool.V0.4.1/SSX MultiTool.exe'))
        tools.add_command(label='Launch EA Graphics Manager',command=lambda:self.launch_tool('EA_Graphics_Manager_v0.45.0/EA_Graphics_Manager/EA-Graphics-Manager-v0.45.0.exe'))
        menu.add_cascade(label='Existing tools',menu=tools)
        menu.add_command(label='Project notes',command=lambda:os.startfile(HOME/'README.md'))
        self.config(menu=menu)
        split = ttk.Panedwindow(self,orient='horizontal')
        split.pack(fill='both',expand=True,padx=10)
        left = ttk.Frame(split,width=360)
        right = ttk.Frame(split)
        split.add(left,weight=1)
        split.add(right,weight=3)
        self.root_label = ttk.Label(left,text=str(self.root_path),wraplength=345)
        self.root_label.pack(fill='x',pady=(0,8))
        self.search = tk.StringVar()
        entry = ttk.Entry(left,textvariable=self.search)
        entry.pack(fill='x')
        self.filter_job = None
        self.search.trace_add('write',self.filter_later)
        ttk.Label(left,text='Filter by name or path · double-click to open').pack(anchor='w',pady=4)
        self.browser = self.table(left,('file','type'),('File / relative path','Type'),(285,55))
        self.browser.bind('<Double-1>',lambda e:self.open_selected_file())
        self.browser.bind('<Return>',lambda e:self.open_selected_file())
        self.file_count = ttk.Label(left,text='')
        self.file_count.pack(anchor='w',pady=5)
        self.title_label = ttk.Label(right,text='EA Playground workbench',font=('Segoe UI',15,'bold'))
        self.title_label.pack(anchor='w',padx=12,pady=(0,5))
        self.path_label = ttk.Label(right,text='Choose an asset in the file browser.',wraplength=800)
        self.path_label.pack(fill='x',padx=12,pady=(0,8))
        self.notebook = ttk.Notebook(right)
        self.notebook.pack(fill='both',expand=True,padx=(10,0))
        self.asset_tab = ttk.Frame(self.notebook)
        self.notebook.add(self.asset_tab,text='Asset')
        self.actions = ttk.Frame(self.asset_tab,padding=6)
        self.actions.pack(fill='x')
        self.content = ttk.Panedwindow(self.asset_tab,orient='vertical')
        self.content.pack(fill='both',expand=True)
        self.preview = Preview(self.content)
        self.list_panel = ttk.Frame(self.content)
        self.content.add(self.preview,weight=3)
        self.content.add(self.list_panel,weight=2)
        self.rows = self.table(self.list_panel,('name','detail','size'),('Name','Details','Count / size'),(340,260,95))
        self.rows.bind('<<TreeviewSelect>>',self.select_row)
        self.rows.bind('<Double-1>',lambda e:self.open_entry() if self.kind=='archive' else None)
        self.details = tk.Text(self.notebook,wrap='word',font=('Consolas',10),padx=12,pady=12)
        self.notebook.add(self.details,text='Details / warnings')
        self.logbox = tk.Text(self.notebook,wrap='word',font=('Consolas',10),padx=12,pady=12)
        self.notebook.add(self.logbox,text='Activity log')
        self.logbox.configure(state='disabled')
        footer = ttk.Frame(self,padding=10)
        footer.pack(side='bottom',fill='x',before=split)
        self.status = tk.StringVar(value='Ready')
        ttk.Label(footer,textvariable=self.status,width=1).pack(side='left',fill='x',expand=True)
        self.progress = ttk.Progressbar(footer,mode='indeterminate',length=140)
        self.progress.pack(side='left',padx=10)
        ttk.Button(footer,text='Cancel job',command=self.cancel.set).pack(side='right')
        self.output.mkdir(parents=True,exist_ok=True)
        self.log('Ready. Source files are read-only; exports go to '+str(self.output))

    def table(self,parent,columns,headings,widths):
        frame = ttk.Frame(parent)
        frame.pack(fill='both',expand=True)
        tree = ttk.Treeview(frame,columns=columns,show='headings',selectmode='extended')
        for c,h,w in zip(columns,headings,widths):
            tree.heading(c,text=h)
            tree.column(c,width=w,minwidth=45)
        scroll = ttk.Scrollbar(frame,orient='vertical',command=tree.yview)
        tree.configure(yscrollcommand=scroll.set)
        scroll.pack(side='right',fill='y')
        tree.pack(fill='both',expand=True)
        return tree

    def log(self,text):
        self.events.put(('log',str(text)))

    def poll(self):
        try:
            while True:
                event = self.events.get_nowait()
                if event[0]=='log':
                    text = time.strftime('%H:%M:%S')+'  '+event[1]+'\n'
                    self.logbox.configure(state='normal')
                    self.logbox.insert('end',text)
                    self.logbox.see('end')
                    self.logbox.configure(state='disabled')
                    with (HOME/'activity.log').open('a',encoding='utf-8') as f:
                        f.write(text)
                elif event[0]=='status':
                    self.status.set(event[1])
                elif event[0]=='done':
                    self.busy=False
                    self.progress.stop()
                    self.status.set('Ready')
                    _,callback,result,error = event
                    if error:
                        self.log(error)
                        messagebox.showerror('Could not complete operation',error.split('\n')[-1]+'\n\nFull details are in Activity log.')
                    elif callback:
                        callback(result)
        except queue.Empty:
            pass
        self.after(60,self.poll)

    def job(self,title,work,done=None):
        if self.busy:
            self.status.set('A job is running. Wait or cancel it before starting another.')
            return False
        self.playing=False
        self.busy=True
        self.cancel.clear()
        self.status.set(title)
        self.progress.start(12)
        self.log(title)
        def run():
            try:
                result=work()
                self.events.put(('done',done,result,None))
            except Exception:
                self.events.put(('done',None,None,traceback.format_exc().strip()))
        threading.Thread(target=run,daemon=True).start()
        return True

    def set_root(self,path):
        if self.busy:
            return
        self.root_path=Path(path)
        self.root_label.config(text=str(path))
        self.scan()

    def choose_root(self):
        path=filedialog.askdirectory(initialdir=self.root_path)
        if path:self.set_root(path)

    def choose_output(self):
        path=filedialog.askdirectory(initialdir=self.output)
        if path:
            self.output=Path(path)
            self.log('Output folder: '+path)

    def scan(self):
        root=self.root_path
        def work():
            if not root.is_dir():raise FileNotFoundError(root)
            found=[]
            for base,dirs,files in os.walk(root):
                dirs[:]=[d for d in dirs if d not in ('__pycache__','.git')]
                if self.cancel.is_set():raise InterruptedError('Folder scan cancelled')
                found.extend(Path(base)/f for f in files)
            return sorted(found,key=lambda p:str(p).lower())
        def done(files):
            self.files=files
            self.filter_files()
            self.log(f'Indexed {len(files):,} files in {root}; unknown formats remain visible')
        self.job('Scanning '+str(root),work,done)

    def filter_later(self,*args):
        if self.filter_job:self.after_cancel(self.filter_job)
        self.filter_job=self.after(180,self.filter_files)

    def filter_files(self):
        self.filter_job=None
        self.browser.delete(*self.browser.get_children())
        query=self.search.get().casefold()
        visible=0
        for i,path in enumerate(self.files):
            name=str(path.relative_to(self.root_path))
            if query in name.casefold():
                self.browser.insert('', 'end',iid=str(i),values=(name,path.suffix))
                visible+=1
        self.file_count.config(text=f'{visible:,} shown / {len(self.files):,} files')

    def open_selected_file(self):
        selected=self.browser.selection()
        if selected:self.open_path(self.files[int(selected[0])])

    def open_dialog(self):
        path=filedialog.askopenfilename(initialdir=self.root_path,filetypes=[('EA assets','*.big *.viv *.o *.gsh *.anm *.ske *.png'),('All files','*.*')])
        if path:self.open_path(Path(path))

    def open_path(self,path):
        path=Path(path)
        def work():
            suffix=path.suffix.lower()
            if suffix in ('.big','.viv'):return 'archive',BigArchive(path)
            if suffix=='.o':
                try:return 'model',core.load_model(path)
                except Exception:return 'inspection',research.inspect(str(path),deep=True)
            if suffix=='.gsh':return 'texture',core.gsh_parser.parse_gsh(core.prepared(path))
            if suffix=='.anm':return 'animation',core.AnimationBank(path)
            if suffix=='.ske':
                try:return 'skeleton',core.load_skeleton(path)
                except Exception:return 'inspection',research.inspect(str(path),deep=True)
            if suffix=='.png':return 'png',path.read_bytes()
            return 'inspection',research.inspect(str(path))
        self.job('Opening '+path.name,work,lambda result:self.show_asset(path,*result))

    def button(self,text,command):
        ttk.Button(self.actions,text=text,command=command).pack(side='left',padx=3)

    def show_details(self,text):
        self.details.configure(state='normal')
        self.details.delete('1.0','end')
        self.details.insert('end',text)
        self.details.configure(state='disabled')

    def show_asset(self,path,kind,asset):
        self.path,self.kind,self.asset=path,kind,asset
        self.clip=None
        self.extra_textures=[]
        self.rows.delete(*self.rows.get_children())
        for child in self.actions.winfo_children():child.destroy()
        self.button('Inspect format',self.inspect_format)
        self.title_label.config(text=path.name)
        self.path_label.config(text=str(path))
        self.preview.set_lines([],kind.capitalize())
        self.notebook.select(self.asset_tab)
        details=''
        if kind=='archive':
            self.button('Open entry',self.open_entry)
            self.button('Extract selected',lambda:self.extract_archive(False))
            self.button('Extract all',lambda:self.extract_archive(True))
            self.unpack=tk.BooleanVar(value=True)
            ttk.Checkbutton(self.actions,text='Decompress RefPack',variable=self.unpack).pack(side='left',padx=8)
            for e in asset.entries:
                self.rows.insert('','end',iid=str(e.index),values=(e.name,'RefPack' if e.compressed else 'Stored',f'{e.size:,} B'))
            details=f'{asset.magic} archive\n{len(asset.entries)} entries\n{sum(e.compressed for e in asset.entries)} RefPack-compressed entries\n\nOpen entry extracts this archive into a new cache folder, preserving sibling textures and skeletons. Extract all/selected writes to a new folder beneath the output folder. Existing files are never overwritten. Nested VIV/BIG archives can be opened in the same way.'
            self.preview.caption=f'{asset.magic} archive · {len(asset.entries):,} entries\nSelect an entry below, then Open entry or Extract.'
            self.preview.draw()
        elif kind=='model':
            self.skeleton=None
            result,materials=asset
            self.button('Export GLB',lambda:self.export_model('glb'))
            self.button('Export OBJ',lambda:self.export_model('obj'))
            self.button('Texture files…',self.choose_textures)
            self.button('Load skeleton…',self.choose_skeleton)
            self.preview.model(result)
            for m in result.meshes:
                self.rows.insert('','end',iid=str(m.index),values=(f'Mesh {m.index}',f'Layout {m.layout} · '+', '.join(sorted(materials.get(m.index,[]))),f'{len(m.faces):,} tris'))
            details=result.summary()+'\n\nGLB embeds matching sibling GSH textures. Choose Texture files to add shared archives. OBJ is geometry only. Character skin export requires Load skeleton. Wireframe preview is untextured; large models are sampled only for display.'
        elif kind=='texture':
            gsh,data=asset
            self.button('Export selected PNG',lambda:self.export_textures(False))
            self.button('Export all PNGs',lambda:self.export_textures(True))
            for i,e in enumerate(gsh.entries):
                note=e.format_label+(' · recovered' if e.recovered else '')
                self.rows.insert('','end',iid=str(i),values=(e.full_name or e.name,note,f'{e.width}×{e.height}'))
            details=f'{gsh.signature} · {gsh.format_ver}\nDirectory entries: {gsh.object_count}\nParsed entries: {len(gsh.entries)}\n\nSupported: CMPR, RGBA8, RGB5A3, PAL4, PAL8. Missing palettes are reported, never exported as misleading grayscale textures.'
        elif kind=='animation':
            self.skeleton=None
            self.button('Load skeleton…',self.choose_skeleton)
            self.button('Play / pause',self.play)
            self.button('Export clip GLB',lambda:self.export_animations(False))
            self.button('Export all clips',lambda:self.export_animations(True))
            self.button('Clip + model…',self.export_animated_model)
            self.slider=ttk.Scale(self.actions,from_=0,to=1,command=self.scrub)
            self.slider.pack(side='left',fill='x',expand=True,padx=6)
            for i,block in enumerate(asset.blocks):
                name=asset.names[i] if i<len(asset.names) else f'clip_{i}'
                self.rows.insert('','end',iid=str(i),values=(name,'Select to decode with skeleton',i))
            details=f'{len(asset.blocks)} indexed clips.\n\nPlayback uses an assumed 30 fps. Clip names resolve through the relocated table_b pointers. Sparse stateless time tables are rejected; extra static channels remain a research gap. Export success is a structural check, not proof that every pose exactly matches the game.\n\nPlayer skeleton export preserves the executable-derived translation mask. A matching skeleton is required.'
            candidate=path.with_name(path.stem.replace('_anims','_skel')+'.ske')
            if candidate.exists():
                self.load_skeleton(candidate)
            elif path.name=='player_anims.anm':
                self.load_skeleton(HOME/'reference/player_skel.ske')
            else:
                self.skeleton=None
        elif kind=='skeleton':
            self.skeleton=asset
            self.button('Export skeleton GLB',self.export_skeleton)
            self.preview.skeleton(asset)
            for b in asset.bones:
                self.rows.insert('','end',values=(b.name,f'Parent {b.parent_idx}',b.index))
            details='\n'.join(asset.log)+'\n\n'+'\n'.join(b.summary() for b in asset.bones)
        elif kind=='png':
            self.preview.texture(asset)
        elif kind=='inspection':
            details=json.dumps(asset,ensure_ascii=False,indent=2,default=research.json_default)
            self.preview.caption=f'{asset["format"]} · {asset["status"]}\nSee Details / warnings for parsed fields and remaining unknowns.'
            self.preview.draw()
            for i,row in enumerate(asset.get('rows',[])[:1000]):
                self.rows.insert('','end',values=(f'Row {i+1}',' | '.join(row),len(row)))
            self.notebook.select(self.details)
        self.show_details(details)
        self.log(f'Opened {kind}: {path}')
        if kind=='texture' and self.rows.get_children():
            self.rows.selection_set(self.rows.get_children()[0])

    def inspect_format(self):
        if not self.path or self.busy:return
        def done(report):
            self.show_details(json.dumps(report,ensure_ascii=False,indent=2,default=research.json_default))
            self.notebook.select(self.details)
        self.job('Inspecting file structure…',lambda:research.inspect(str(self.path),deep=True),done)

    def selected_indices(self):
        return [int(i) for i in self.rows.selection()]

    def select_row(self,event=None):
        indices=self.selected_indices() if self.kind in ('archive','model','texture','animation') else []
        if not indices or self.busy:return
        index=indices[0]
        if self.kind=='texture':
            gsh,data=self.asset
            entry=gsh.entries[index]
            self.job('Decoding '+(entry.full_name or entry.name),lambda:self.texture_png(entry,data),self.preview.texture)
        elif self.kind=='animation':
            if self.skeleton is None:
                self.status.set('Load a matching .ske skeleton to decode and play the clip.')
                return
            bank,skel=self.asset,self.skeleton
            def done(clip):
                self.clip=clip
                self.frame=0
                self.rows.item(str(index),values=(clip.name,getattr(clip,'codec','Decoded')+f' · {(clip.sample_count-1)/30:.2f}s',f'{clip.sample_count} frames'))
                self.slider.configure(to=max(clip.sample_count-1,1))
                self.slider.set(0)
                self.preview.skeleton(skel,clip,0)
                self.log(f'Clip {index}: {clip.sample_count} samples; '+ '; '.join(clip.get('caveats',[]) if isinstance(clip,dict) else getattr(clip,'caveats',[])))
            self.job(f'Decoding clip {index}',lambda:bank.decode(index,skel),done)

    @staticmethod
    def texture_png(entry,data):
        if entry.record_id in (24,25) and not entry.palette:
            raise ValueError(f'{entry.full_name or entry.name}: missing palette; accurate color decode unavailable')
        return core.gsh_parser.decode_entry_png_bytes(entry,data)

    def unique_folder(self,label):
        base=self.output/core.filename(label)
        path=base
        number=2
        while path.exists():
            path=base.with_name(base.name+f'_{number}')
            number+=1
        path.mkdir(parents=True)
        return path

    def extract_archive(self,all_entries):
        if self.busy:return
        archive=self.asset
        entries=archive.entries if all_entries else [archive.entries[i] for i in self.selected_indices()]
        if not entries:return
        unpack=self.unpack.get()
        def work():
            folder=self.unique_folder(self.path.stem+'_extracted')
            archive.extract(folder,entries,unpack,lambda i,n,name:self.events.put(('status',f'Extracting {i}/{n}: {name}')),self.cancel.is_set)
            return folder
        def done(folder):
            self.log('Extracted to '+str(folder))
            self.set_root(folder)
        self.job('Extracting archive…',work,done)

    def open_entry(self):
        if self.kind!='archive' or self.busy:return
        indices=self.selected_indices()
        if not indices:return
        archive=self.asset
        entry=archive.entries[indices[0]]
        def work():
            import uuid
            from archives import safe_target
            cache=HOME/'cache'
            cache.mkdir(exist_ok=True)
            folder=cache/(core.filename(archive.path.stem)+'_'+uuid.uuid4().hex[:12])
            folder.mkdir()
            archive.extract(folder,cancelled=self.cancel.is_set)
            return safe_target(folder,entry.name)
        self.job('Preparing archive and sibling assets…',work,self.open_path)

    def choose_textures(self):
        paths=filedialog.askopenfilenames(initialdir=self.path.parent,filetypes=[('GSH textures','*.gsh')])
        if paths:
            self.extra_textures=[Path(p) for p in paths]
            self.log(f'Added {len(paths)} texture archives for model export')

    def choose_skeleton(self):
        path=filedialog.askopenfilename(initialdir=self.path.parent if self.path else HOME/'reference',filetypes=[('EA skeleton','*.ske')])
        if path:self.load_skeleton(Path(path))

    def load_skeleton(self,path):
        def done(skel):
            self.skeleton=skel
            self.clip=None
            self.log(f'Loaded skeleton: {path} ({len(skel.bones)} bones)')
            if self.kind=='animation':
                self.preview.skeleton(skel)
                self.select_row()
        self.job('Loading skeleton '+path.name,lambda:core.load_skeleton(path),done)

    def export_model(self,format):
        if self.busy:return
        result,materials=self.asset
        path=self.path
        skel=self.skeleton
        textures=list(path.parent.glob('*.gsh'))+self.extra_textures
        textures=list(dict.fromkeys(textures))
        def work():
            data=core.model_obj(result) if format=='obj' else core.model_glb(path,result,materials,skel,textures=textures,log=self.log)
            if format=='glb':core.validate_glb(data)
            folder=self.unique_folder(path.stem)
            return core.write_new(folder/(path.stem+'.'+format),data)
        self.job('Exporting model…',work,lambda p:self.log('Exported '+str(p)))

    def export_textures(self,all_entries):
        if self.busy:return
        gsh,data=self.asset
        indices=list(range(len(gsh.entries))) if all_entries else self.selected_indices()
        if not indices:return
        def work():
            folder=self.unique_folder(self.path.stem+'_textures')
            report=[]
            for i in indices:
                if self.cancel.is_set():break
                e=gsh.entries[i]
                try:
                    png=self.texture_png(e,data)
                    core.write_new(folder/(f'{i:03d}_'+core.filename(e.full_name or e.name)+'.png'),png)
                    report.append({'entry':i,'name':e.full_name or e.name,'status':'ok'})
                except Exception as exc:
                    report.append({'entry':i,'status':'error','error':str(exc)})
            core.write_new(folder/'report.json',json.dumps(report,indent=2).encode())
            return f'{sum(r["status"]=="ok" for r in report)}/{len(indices)} textures exported to {folder}; see report.json for failures/cancellation.'
        self.job('Exporting PNG textures…',work,self.log)

    def export_animations(self,all_clips):
        if self.busy:return
        if self.skeleton is None:
            messagebox.showinfo('Skeleton required','Load the matching .ske file first.')
            return
        bank,skel=self.asset,self.skeleton
        indices=list(range(len(bank.blocks))) if all_clips else self.selected_indices()
        if not indices:return
        def work():
            folder=self.unique_folder(self.path.stem+'_clips')
            report=[]
            for i in indices:
                if self.cancel.is_set():break
                self.events.put(('status',f'Exporting clip {len(report)+1}/{len(indices)}'))
                try:
                    clip=bank.decode(i,skel)
                    data=bank.export(clip,skel)
                    core.validate_glb(data)
                    core.write_new(folder/(f'{i:03d}_'+core.filename(clip.name)+'.glb'),data)
                    report.append({'index':i,'name':clip.name,'status':'ok','samples':clip.sample_count,'caveats':getattr(clip,'caveats',[])})
                except Exception as exc:
                    report.append({'index':i,'status':'error','error':str(exc)})
            core.write_new(folder/'report.json',json.dumps({'assumed_fps':30,'requested':len(indices),'cancelled':self.cancel.is_set(),'clips':report},indent=2).encode())
            return f'{sum(r["status"]=="ok" for r in report)}/{len(indices)} clips exported to {folder}; see report.json.'
        self.job('Exporting animation clips…',work,self.log)

    def export_skeleton(self):
        skel=self.skeleton
        def work():
            data=core.eagl_skeleton.build_skeleton_gltf(skel)
            core.validate_glb(data)
            return core.write_new(self.unique_folder(self.path.stem)/(self.path.stem+'.glb'),data)
        self.job('Exporting skeleton…',work,lambda p:self.log('Exported '+str(p)))

    def export_animated_model(self):
        if self.busy or self.clip is None or self.skeleton is None:
            self.status.set('Select and decode an animation clip first.')
            return
        path=filedialog.askopenfilename(initialdir=DEFAULT_OLD/'Characters',filetypes=[('EA model','*.o')])
        if not path:return
        path=Path(path)
        clip,skel=self.clip,self.skeleton
        def work():
            result,materials=core.load_model(path)
            data=core.model_glb(path,result,materials,skel,clip,log=self.log)
            doc=core.validate_glb(data)
            if not doc.get('skins') or not doc.get('animations'):
                raise ValueError('Model has no usable skin or animation; choose a matching character model')
            folder=self.unique_folder(path.stem+'_animated')
            return core.write_new(folder/(core.filename(clip.name)+'.glb'),data)
        self.job('Exporting model with selected animation…',work,lambda p:self.log('Exported '+str(p)))

    def play(self):
        if self.busy or not self.clip:return
        self.playing=not self.playing
        if self.playing:self.tick()

    def tick(self):
        if not self.playing or self.kind!='animation' or not self.clip:return
        self.frame=(self.frame+1)%self.clip.sample_count
        self.slider.set(self.frame)
        self.after(33,self.tick)

    def scrub(self,value):
        if self.clip and self.skeleton:
            self.frame=min(int(float(value)),self.clip.sample_count-1)
            self.preview.skeleton(self.skeleton,self.clip,self.frame,fit=False)

    def launch_tool(self,relative):
        path=SOURCE/'extraction tools'/relative
        if not path.exists():
            messagebox.showerror('Tool not found',str(path))
            return
        subprocess.Popen([str(path)],cwd=path.parent)

    def close(self):
        if self.busy:
            self.cancel.set()
            self.status.set('Cancelling job; close again after the current operation finishes.')
            return
        (HOME/'settings.json').write_text(json.dumps({'data_root':str(self.root_path),'output':str(self.output)},indent=2),encoding='utf-8')
        self.destroy()

    def report_callback_exception(self,exc,value,tb):
        self.log(''.join(traceback.format_exception(exc,value,tb)))
        messagebox.showerror('UI error',str(value)+'\nSee Activity log for details.')


def main():
    parser=argparse.ArgumentParser()
    parser.add_argument('--smoke-test',action='store_true')
    args=parser.parse_args()
    app=App()
    if args.smoke_test:
        def finish():
            if app.busy:
                app.after(100,finish)
                return
            checks={'title':app.title(),'indexed_files':len(app.files),'tk':tk.TkVersion,'frozen':getattr(sys,'frozen',False),'status':'ok'}
            try:
                skel=core.eagl_skeleton.parse_ske_file(HOME/'reference/player_skel.ske')
                bank=core.AnimationBank(HOME/'reference/player_anims.anm')
                clip=bank.decode(0,skel)
                core.validate_glb(bank.export(clip,skel))
                checks['clips']=len(bank.blocks)
                path=DEFAULT_OLD/'WorldProps/basketball.o'
                if path.exists():
                    result,mats=core.load_model(path)
                    glb=core.model_glb(path,result,mats)
                    doc=core.validate_glb(glb)
                    checks['model_triangles']=result.total_faces
                    checks['embedded_images']=len(doc.get('images',[]))
            except Exception:
                checks['status']='error'
                checks['error']=traceback.format_exc()
            (HOME/'ui-smoke-test.json').write_text(json.dumps(checks,indent=2))
            app.destroy()
        app.after(600,finish)
    app.mainloop()


if __name__=='__main__':
    try:
        main()
    except Exception:
        (HOME/'startup-error.log').write_text(traceback.format_exc(),encoding='utf-8')
        raise
