"""Differential check of the Rust PowerPC disassembler (src/ppc.rs) against Capstone.

Usage:  py -3.14 tools/ppc_oracle.py <listing.s>
The listing is the `.s` output of `EAGL-Workbench.exe --decode-all` for playgroundz.elf (or an AEMS bank).  Every
instruction word is re-disassembled by Capstone and both texts are reduced to a canonical form; only notation that
is provably equivalent is folded (simplified mnemonics, register-0 base spelling, SPR names/numbers, CR-bit names).
Paired-single/quantized opcodes (4, 56, 57, 60, 61) are not supported by Capstone and are counted separately, as are
words Capstone rejects (e.g. fcmpo).  Exits nonzero on any real disagreement.
"""
import sys,re,collections
from pathlib import Path
sys.path[:0]=[str(Path(__file__).resolve().parents[2]/'Remaster'/'research'/'deps')]
import capstone

SPR={'xer':1,'lr':8,'ctr':9,'dsisr':18,'dar':19,'dec':22,'sdr1':25,'srr0':26,'srr1':27,'ear':282,'tbl':284,'tbu':285,'pvr':287,
     'hid2':920,'ummcr0':936,'wpar':921,'dma_u':922,'dma_l':923,'mmcr0':952,'pmc1':953,'pmc2':954,'sia':955,'mmcr1':956,'pmc3':957,'pmc4':958,
     'hid0':1008,'hid1':1009,'iabr':1010,'dabr':1013,'l2cr':1017,'ictc':1019,'iccr':1019,'thrm1':1020,'thrm2':1021,'thrm3':1022,'esr':980,'dear':981}
for i in range(4):SPR[f'sprg{i}']=272+i
for i in range(4):SPR[f'ibat{i}u']=528+2*i;SPR[f'ibat{i}l']=529+2*i;SPR[f'dbat{i}u']=536+2*i;SPR[f'dbat{i}l']=537+2*i
for i in range(8):SPR[f'gqr{i}']=912+i

def canon(t):
    t=t.lower().strip();t=re.sub(r'\s+',' ',t)
    m=t.split(' ',1);mn=m[0];ops=[o.strip() for o in m[1].split(',')] if len(m)>1 else []
    def num(x):
        x=x.strip()
        if re.fullmatch(r'-?0x[0-9a-f]+',x):return str(int(x,16))
        if re.fullmatch(r'-?\d+',x):return str(int(x))
        return x
    ops=[num(o) for o in ops]
    ops=[re.sub(r'(-?0x[0-9a-f]+|-?\d+)\((r0|0)\)',lambda q:num(q.group(1))+'(0)',o) for o in ops]
    ops=[re.sub(r'(-?0x[0-9a-f]+)\(',lambda q:num(q.group(1))+'(',o) for o in ops]
    # CR bit names -> numbers
    def crbit(o):
        q=re.fullmatch(r'cr(\d)(lt|gt|eq|un|so)',o)
        return str(int(q.group(1))*4+['lt','gt','eq','so','un'].index(q.group(2))%4) if q else o
    ops=[crbit(o) for o in ops]
    if ops and ops[0]=='cr0' and len(ops)>1:ops=ops[1:]
    # simplified forms -> canonical
    if mn in('crclr','crset'):mn,ops={'crclr':'crxor','crset':'creqv'}[mn],[ops[0]]*3
    if mn.rstrip('.') in('slwi','srwi','clrrwi','clrlwi','rotlwi'):
        dot='.' if mn.endswith('.') else '';b=mn.rstrip('.');ra,rs,n=ops;n=int(n)
        sh,mb,me={'slwi':(n,0,31-n),'srwi':((32-n)%32,n,31),'clrrwi':(0,0,31-n),'clrlwi':(0,n,31),'rotlwi':(n,0,31)}[b]
        mn,ops='rlwinm'+dot,[ra,rs,str(sh),str(mb),str(me)]
    if mn.rstrip('.')=='mr':mn,ops='or'+('.' if mn.endswith('.') else ''),[ops[0],ops[1],ops[1]]
    if mn.rstrip('.')=='not':mn,ops='nor'+('.' if mn.endswith('.') else ''),[ops[0],ops[1],ops[1]]
    if mn in('mftb',) and len(ops)==2:ops=ops[:1]
    q=re.fullmatch(r'm([tf])(ibat|dbat)([ul])',mn)
    if q:
        spr=SPR[f'{q.group(2)}{ops[0] if q.group(1)=="t" else ops[1]}{q.group(3)}']
        mn,ops=('mtspr',[str(spr),ops[1]]) if q.group(1)=='t' else ('mfspr',[ops[0],str(spr)])
    q=re.fullmatch(r'm([tf])(dsisr|dar|pvr|tbl|tbu|iccr|esr|dear|srr0|srr1|sdr1|dec|ear)',mn)
    if q:
        spr=SPR[q.group(2)];mn,ops=('mtspr',[str(spr),ops[0]]) if q.group(1)=='t' else ('mfspr',[ops[0],str(spr)])
    if mn in('mtspr','mfspr'):
        i=0 if mn=='mtspr' else 1;s=ops[i]
        if s in SPR:ops[i]=str(SPR[s])
        elif s.startswith('spr'):ops[i]=str(int(s[3:]))
    # register 0 as an address operand (indexed forms) is literal 0
    if mn in('dcbst','dcbf','dcbi','dcbt','dcbtst','dcbz','icbi') and ops and ops[0]=='r0':ops[0]='0'
    if re.fullmatch(r'(l|st)[a-z]*x|lwbrx|lhbrx|stwbrx|sthbrx|lswx|stswx|eciwx|ecowx|lwarx|stwcx\.',mn) and len(ops)==3 and ops[1]=='r0':ops[1]='0'
    return mn+' '+','.join(ops)

def main(path):
    cs=capstone.Cs(capstone.CS_ARCH_PPC,capstone.CS_MODE_32|capstone.CS_MODE_BIG_ENDIAN)
    stats=collections.Counter();bad=collections.Counter();ex={}
    for line in open(path,encoding='utf-8'):
        m=re.match(r'([0-9a-f]{8}): ([0-9a-f]{8})  (.*?)(?: ; .*)?$',line)
        if not m:continue
        a=int(m.group(1),16);w=bytes.fromhex(m.group(2));ours=m.group(3)
        if w[0]>>2 in(4,56,57,60,61):stats['paired_single_not_in_capstone']+=1;continue
        ins=next(cs.disasm(w,a),None)
        if ins is None:stats['capstone_rejects_'+ours.split()[0]]+=1;continue
        if canon(ours)==canon(f'{ins.mnemonic} {ins.op_str}'):stats['agree']+=1;continue
        k=(ours.split()[0],ins.mnemonic);bad[k]+=1;ex.setdefault(k,(m.group(2),ours,f'{ins.mnemonic} {ins.op_str}'))
    for k,v in sorted(stats.items()):print(f'{k}: {v}')
    print('disagree:',sum(bad.values()))
    for k,v in bad.most_common(20):print(' ',v,k,ex[k])
    return 1 if bad else 0

if __name__=='__main__':sys.exit(main(sys.argv[1]))
