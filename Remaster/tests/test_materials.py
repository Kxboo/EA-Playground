"""Material identity, duplicate/conflict handling and real dependency fixtures."""
import sys,json,copy,unittest
from pathlib import Path
from types import SimpleNamespace
from unittest.mock import patch
ROOT=Path(__file__).resolve().parents[1]
sys.path[:0]=[str(ROOT/'src'),str(ROOT.parent/'_bevy/tools')]
import core,material_bindings,decoder_bridge as bridge
from asset_links import texture_sources
from audit_corpus import check_glb

class ResolutionTests(unittest.TestCase):
    def resolve(self,entries,name='wood'):
        report=[];warnings=[]
        with patch.object(core,'prepared',side_effect=lambda p:p),patch.object(core.gsh_parser,'parse_gsh',return_value=(SimpleNamespace(entries=entries),b'')),patch.object(core.gsh_parser,'decode_entry_rgba',side_effect=lambda e,d:(e.rgba,1,1)):
            images,modes=core.texture_images({0:{name}},[Path('bank.gsh')],warnings.append,report)
        return images,report,warnings

    def entry(self,index,rgba,name='wood',short='wood'):
        return SimpleNamespace(index=index,rgba=bytes(rgba),full_name=name,name=short,record_id=5)

    def test_identical_duplicates_are_one_image(self):
        images,report,warnings=self.resolve([self.entry(0,[1,2,3,255]),self.entry(1,[1,2,3,255])])
        self.assertEqual(list(images),['wood']);self.assertFalse(warnings)
        self.assertEqual(report[0]['distinct_images'],1);self.assertEqual(len(report[0]['sources']),2)

    def test_different_images_stay_ambiguous(self):
        images,report,warnings=self.resolve([self.entry(0,[1,2,3,255]),self.entry(1,[9,2,3,255])])
        self.assertFalse(images);self.assertEqual(report[0]['status'],'ambiguous')
        self.assertEqual(report[0]['distinct_images'],2);self.assertTrue(warnings)

    def test_full_name_prevents_prefix_false_match(self):
        images,report,_=self.resolve([self.entry(0,[1,2,3,255],name='wood_different')],'wood_missing')
        self.assertFalse(images);self.assertEqual(report[0]['status'],'missing')

    def test_shader_fields_match_executable_schema(self):
        schemas=json.loads((ROOT/'research/shader-schemas.json').read_text())['schemas']
        for name,offsets in material_bindings.TEXTURES.items():
            self.assertEqual(offsets,tuple(f['pointer_offset'] for f in schemas[name] if f['name']=='Texture' or f['name'] in ('Texture1','Texture2','Texture3')),name)

    def test_npot_wrap_is_clamped(self):
        mesh=SimpleNamespace(material_bindings=[{'symbol':';22=1;23=2;'}])
        png=core.encode(bytes([255,255,255,255])*30*32,30,32)
        self.assertEqual(material_bindings.sampler(mesh,png),{'wrapS':33071,'wrapT':33648})

    def test_vertex_colour_seams_survive_normal_remapping(self):
        import struct
        raw=bytes([255,0,0,255,0,0,255,255])+struct.pack('>I',2)
        mesh=SimpleNamespace(positions=[(0.,0.,0.),(1.,0.,0.),(0.,1.,0.)],
            faces=[((0,0,0),(1,0,1),(2,0,2)),((0,1,0),(2,1,2),(1,1,1))],source_arrays={'normals':0})
        material_bindings.vertex_colours(mesh,raw,0,12,{12:0})
        a,b=mesh.faces[0][0],mesh.faces[1][0]
        self.assertNotEqual(a,b)
        self.assertEqual(mesh.vertex_colors[a],(1.,0.,0.,1.))
        self.assertEqual(mesh.vertex_colors[b],(0.,0.,1.,1.))
        self.assertTrue(mesh.material_unlit)

@unittest.skipUnless(core.DEFAULT_DATA.exists(),'local game DATA unavailable')
class MaterialFixtures(unittest.TestCase):
    @classmethod
    def setUpClass(cls):cls.rows=bridge.catalog()['assets']
    def source(self,name):return next(r['source'] for r in self.rows if r['name']==name)

    def test_net_uses_shader_binding_not_tail_metadata(self):
        p=bridge.local(self.source('net.o'));result,materials=core.load_model(p)
        self.assertEqual(materials,{0:{'metal_yellow'},1:{'net'}})
        warnings=[];report=[]
        doc=check_glb(core.model_glb(p,result,materials,textures=[bridge.local(s) for s in texture_sources(self.source('net.o'))],log=warnings.append,material_report=report))
        self.assertFalse(warnings);self.assertEqual(len(doc['images']),2)
        self.assertEqual(doc['materials'][1]['name'],'net')
        self.assertIn(doc['materials'][1]['alphaMode'],('MASK','BLEND'))

    def test_toon_normals_are_s16_q14(self):
        import struct
        from containers import Elf
        path=bridge.local(self.source('net.o'));result,_=core.load_model(path)
        elf=Elf(path.read_bytes());raw=elf.section_bytes(elf.section('.data'))
        for mesh in result.meshes:
            offset=mesh.source_arrays['normals']
            expected=tuple(x/16384 for x in struct.unpack_from('>3h',raw,offset))
            self.assertEqual(mesh.normals[0],expected)
            self.assertTrue(all(abs(sum(x*x for x in n)-1)<.005 for n in mesh.normals))

    def test_world_archive_finds_loose_banks(self):
        banks=texture_sources(self.source('world-low-all.o'))
        self.assertEqual({Path(s).name for s in banks},{'world.gsh','world-misc.gsh'})
        self.assertTrue(all('worldprops' not in s for s in banks))

    def test_early_descriptor_still_gets_exact_shader_material(self):
        result,materials=core.load_model(bridge.local(self.source('playground-high.o')))
        self.assertTrue(all(hasattr(m,'shader_family') and len(materials[m.index])==1 for m in result.meshes if m.ok))
        self.assertTrue(all(m.vertex_colors and m.material_unlit for m in result.meshes if m.ok))
        self.assertFalse([w for m in result.meshes for w in m.warnings if w.startswith('Material ')])

    def test_shadow_is_intentionally_textureless(self):
        p=bridge.local(self.source('rc_buggybody_shadow.o'));result,materials=core.load_model(p)
        warnings=[];report=[]
        core.model_glb(p,result,materials,textures=[],log=warnings.append,material_report=report)
        self.assertTrue(all(m.textureless for m in result.meshes));self.assertFalse(report);self.assertFalse(warnings)

    def test_sampler_changes_do_not_merge_materials(self):
        p=bridge.local(self.source('basketball.o'));result,materials=core.load_model(p)
        duplicate=copy.deepcopy(result.meshes[0]);duplicate.index=1
        duplicate.material_bindings[0]['symbol']=';1=basketball,1;22=0;23=0;'
        result.meshes.append(duplicate);materials[1]={'basketball'}
        doc=check_glb(core.model_glb(p,result,materials,textures=[bridge.local(s) for s in texture_sources(self.source('basketball.o'))]))
        self.assertEqual(len(doc['images']),1);self.assertEqual(len(doc['materials']),2)
        self.assertEqual({s['wrapS'] for s in doc['samplers']},{33071,10497})

if __name__=='__main__':unittest.main()
