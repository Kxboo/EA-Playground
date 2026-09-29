"""Regression checks for recovered format defects, including real game fixtures."""
import sys,struct,unittest
from pathlib import Path
sys.path.insert(0,str(Path(__file__).resolve().parents[1]/'src'))
import core,research
from formats import csv_table,tpl,tpl_rgba
from audit_corpus import check_glb


class ResearchTests(unittest.TestCase):
    def test_quoted_tsv(self):
        r=csv_table(b'key\tvalue\r\n1\t"comma, quote ""here""\nand newline"\r\n')
        self.assertEqual(r['delimiter'],'\t')
        self.assertEqual(r['rows'][1],['1','comma, quote "here"\nand newline'])

    def test_split_palette_alignment(self):
        header=bytes.fromhex('33000050000300010100000020000000')
        palette=bytearray(64)
        palette[:6]=bytes((255,10,128,20,0,30))
        palette[32:38]=bytes((40,50,60,70,80,90))
        self.assertEqual(core.gsh_parser._try_read_palette(header+palette,0),
                         [(10,40,50,255),(20,60,70,128),(30,80,90,0)])

    def test_narrow_gx_tiles(self):
        g=core.gsh_parser
        self.assertEqual(g._mip_chain(4,25,240,1,1),[(4,25,224)])
        colors=[(0,0,0,255),(200,10,5,255)]
        raw=bytes(192)+bytes([1])*32
        pixels=g.decode_pal8(raw,4,25,colors)
        self.assertEqual(pixels[-16:],bytes(colors[1])*4)
        self.assertEqual(pixels[:16],bytes(colors[0])*4)

    def test_tpl_rejects_truncation(self):
        data=struct.pack('>III',0x20af30,1,12)+struct.pack('>II',20,0)+struct.pack('>HHII',4,4,6,32)
        with self.assertRaises(ValueError):tpl(data)

    @unittest.skipUnless(core.DEFAULT_DATA.exists(),'local game DATA unavailable')
    def test_warning_texture_24bit_size(self):
        g,data=core.gsh_parser.parse_gsh(core.DEFAULT_DATA/'files/data/boot/strapwarn_standard_english.gsh')
        e=g.entries[0]
        self.assertEqual(e.size_of_block,0x4b020)
        self.assertEqual(e.full_name,'strapA_screen')
        self.assertEqual(len(core.gsh_parser.decode_entry_rgba(e,data)[0]),640*480*4)
        self.assertFalse(e.recovered)

    @unittest.skipUnless(core.DEFAULT_OLD.exists(),'old game fixtures unavailable')
    def test_rc_skeleton_and_animation(self):
        folder=core.DEFAULT_OLD/'placeables/rc_trackcar'
        skel=core.load_skeleton(folder/'rc_track_car_skel.ske')
        self.assertEqual([b.parent_idx for b in skel.bones],[-1,0])
        bank=core.AnimationBank(folder/'rc_track_car_anims.anm')
        clip=bank.decode(0,skel)
        doc=check_glb(bank.export(clip,skel))
        self.assertEqual(len(doc['nodes']),2)
        self.assertTrue(doc['animations'][0]['channels'])

    def test_player_scale_preserved(self):
        bank=core.AnimationBank(core.HOME/'reference/player_anims.anm')
        skel=core.load_skeleton(core.HOME/'reference/player_skel.ske')
        index=bank.names.index('S_sk_bag_reach')
        clip=bank.decode(index,skel)
        self.assertTrue(clip.scale_by_bone)
        doc=check_glb(bank.export(clip,skel))
        self.assertIn('scale',{c['target']['path'] for c in doc['animations'][0]['channels']})

    def test_nested_archive_read(self):
        source=str(core.DEFAULT_DATA/'files/data/fe/main.big')+'::Main.gsh'
        if not core.DEFAULT_DATA.exists():self.skipTest('local game DATA unavailable')
        r=research.inspect(source,True)
        self.assertTrue(r['payload']['images'])
        self.assertTrue(all(i['status']=='decoded_pixels' for i in r['payload']['images']))


if __name__=='__main__':unittest.main()
