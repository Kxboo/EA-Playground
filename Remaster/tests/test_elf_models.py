"""Tests for executable-confirmed PCode and recovered model families."""
import sys,json,struct,unittest
from pathlib import Path
ROOT=Path(__file__).resolve().parents[1]
sys.path[:0]=[str(ROOT/'src'),str(ROOT.parent/'_bevy/tools')]
import core,research,frontend_models,decoder_bridge as bridge
from pcode import decode
from audit_corpus import check_glb

class PCodeTests(unittest.TestCase):
    def test_mixed_width_and_fraction(self):
        code=bytes.fromhex('05 00 01 00 00 00 00 04 00 07 00000000 00000020 0a00 0b0d 0909 080b 090d 00')
        result=decode(code,0,{10:123})
        self.assertEqual((result['offset'],result['length'],result['fraction']),(123,32,13))
        self.assertEqual(result['attributes'],{0:('direct',1),9:('index',2),11:('index',1),13:('index',2)})

    def test_truncation_and_unrecognized_operation(self):
        for code in (b'\x05\0\1',b'\x07\0',b'\xff',b'\x08\x09\x09\x09\0'):
            with self.subTest(code=code),self.assertRaises(ValueError):decode(code,0,{})

    def test_bounded_gx_indices(self):
        with self.assertRaises(ValueError):frontend_models.triangles(bytes.fromhex('900003000103'),1,3)
        with self.assertRaises(ValueError):frontend_models.triangles(bytes.fromhex('9000030001'),1,3)

@unittest.skipUnless(core.DEFAULT_DATA.exists(),'local game DATA unavailable')
class RealModels(unittest.TestCase):
    @classmethod
    def setUpClass(cls):
        cls.rows=json.loads((ROOT/'research/coverage.json').read_text())['records']

    def source(self,name,contains=''):
        return next(r['source'] for r in self.rows if r['name']==name and contains in r['source'])

    def test_frontend_uv_and_export_does_not_mutate(self):
        source=self.source('HelpEditor.o');path=bridge.local(source)
        result,materials=core.load_model(path)
        images,_=core.texture_images(materials,[bridge.local(t) for t in bridge.siblings(source,'.gsh')])
        self.assertEqual(list(images),['1']) # source short name is space-padded
        self.assertEqual(struct.unpack('>II',images['1'][16:24]),(30,30))
        raw=list(result.meshes[0].uvs)
        converted=frontend_models.prepare_export(result,images,lambda _:None)
        self.assertLess(max(x for uv in converted.meshes[0].uvs for x in uv),1.001)
        self.assertGreater(max(x for uv in raw for x in uv),29.9)
        blob=core.model_glb(path,result,materials,textures=[bridge.local(t) for t in bridge.siblings(source,'.gsh')])
        doc=check_glb(blob)
        self.assertIn('KHR_materials_unlit',doc['extensionsUsed'])
        self.assertEqual(result.meshes[0].uvs,raw)
        self.assertEqual(blob,core.model_glb(path,result,materials,textures=[bridge.local(t) for t in bridge.siblings(source,'.gsh')]))

    def test_solid_frontend(self):
        path=bridge.local(self.source('NunchuckRequired_English.o','16_9'))
        result,materials=core.load_model(path)
        self.assertEqual(result.total_faces,4532)
        self.assertTrue(all(m.apt_texture is None for m in result.meshes))
        check_glb(core.model_glb(path,result,materials,textures=[]))

    def test_rc_model_precision_skin_and_colors(self):
        source=self.source('rc_track_car.o');path=bridge.local(source)
        result,materials=core.load_model(path)
        self.assertEqual((len(result.meshes),result.total_faces),(5,980))
        self.assertEqual({m.position_fraction for m in result.meshes},{13})
        skel=core.load_skeleton(bridge.local(self.source('rc_track_car_skel.ske')))
        bank=core.AnimationBank(bridge.local(self.source('rc_track_car_anims.anm')))
        doc=check_glb(core.model_glb(path,result,materials,skeleton=skel,clip=bank.decode(0,skel),textures=[]))
        for mesh in doc['meshes']:
            for p in mesh['primitives']:
                self.assertIn('COLOR_0',p['attributes']);self.assertIn('JOINTS_0',p['attributes'])
        self.assertEqual(len(doc['skins'][0]['joints']),2)

    def test_zero_offset_weight_table_and_empty_drawlists(self):
        result,_=core.load_model(bridge.local(self.source('shadow_shadow.o')))
        self.assertEqual(len(result.meshes[0].bone_weights),3)
        self.assertEqual(result.meshes[0].bone_weights[0][3],31)
        source=self.source('rc_track_car_shadow.o')
        report=research.inspect(source,True)['payload']
        self.assertEqual(report['status'],'structural')
        self.assertTrue(report['empty_draw_lists'])
        self.assertEqual(bridge.preview({'source':source})['kind'],'inspection')

if __name__=='__main__':unittest.main()
