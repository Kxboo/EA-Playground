//! PowerPC 750CL (Gekko/Broadway) disassembler: the full user-mode integer, floating-point, load/store, branch,
//! condition-register and cache instructions, the supervisor instructions the game uses (`mtmsr`, `mfspr`, `rfi`,
//! ...), and Gekko paired-single / quantized load-store (`psq_*`, `ps_*`).  Output follows the usual simplified
//! mnemonics (`li`, `lis`, `mr`, `nop`, `blr`, `beq`, `slwi`, `clrlwi`, `mtlr`, ...).
//!
//! Used to decode executable code in the corpus: the symbol-bearing `playgroundz.elf` (every function) and the
//! AEMS module banks (`.abk`), whose sound-event classes are compiled PowerPC routines.

fn sext16(v:u32)->i32{(v as u16) as i16 as i32}
fn fmt_simm(v:i32)->String{if v<0{format!("-{:#x}",-(v as i64))}else{format!("{v:#x}")}}
fn spr_name(n:u32)->String{
    match n{1=>"xer".into(),8=>"lr".into(),9=>"ctr".into(),18=>"dsisr".into(),19=>"dar".into(),22=>"dec".into(),25=>"sdr1".into(),26=>"srr0".into(),27=>"srr1".into(),
        272..=275=>format!("sprg{}",n-272),282=>"ear".into(),284=>"tbl".into(),285=>"tbu".into(),287=>"pvr".into(),
        528..=535=>format!("ibat{}{}",(n-528)/2,if n%2==0{"u"}else{"l"}),536..=543=>format!("dbat{}{}",(n-536)/2,if n%2==0{"u"}else{"l"}),
        912..=919=>format!("gqr{}",n-912),920=>"hid2".into(),921=>"wpar".into(),922=>"dma_u".into(),923=>"dma_l".into(),936=>"ummcr0".into(),
        952=>"mmcr0".into(),953=>"pmc1".into(),954=>"pmc2".into(),955=>"sia".into(),956=>"mmcr1".into(),957=>"pmc3".into(),958=>"pmc4".into(),
        1008=>"hid0".into(),1009=>"hid1".into(),1010=>"iabr".into(),1013=>"dabr".into(),1017=>"l2cr".into(),1019=>"ictc".into(),1020=>"thrm1".into(),1021=>"thrm2".into(),1022=>"thrm3".into(),
        _=>format!("spr{n}")}
}

/// One decoded instruction: mnemonic, operand text and (for relative branches) the absolute target.
pub struct Ins{pub mnemonic:String,pub operands:String,pub target:Option<u32>}
impl Ins{
    fn new(m:&str,o:String)->Self{Ins{mnemonic:m.into(),operands:o,target:None}}
    pub fn text(&self)->String{if self.operands.is_empty(){self.mnemonic.clone()}else{format!("{} {}",self.mnemonic,self.operands)}}
}

fn cond_name(bo:u32,bi:u32)->Option<String>{
    // Simplified conditional mnemonics for the common BO encodings.
    let cr=bi/4;let bit=bi%4;
    let (t,f)=match bit{0=>("lt","ge"),1=>("gt","le"),2=>("eq","ne"),_=>("so","ns")};
    let base=match bo&0x1e{12|14=>Some(t),4|6=>Some(f),_=>None}?;
    Some(if cr==0{base.to_string()}else{format!("{base} cr{cr},")})
}

pub fn disasm(w:u32,addr:u32)->Ins{
    let op=w>>26;let rd=(w>>21)&31;let ra=(w>>16)&31;let rb=(w>>11)&31;let rc=(w>>6)&31;
    let uimm=w&0xffff;let simm=sext16(w);
    let r=|n:u32|format!("r{n}");let f=|n:u32|format!("f{n}");
    let mem=|d:i32,a:u32|format!("{}(r{a})",fmt_simm(d));
    let rcs=if w&1!=0{"."}else{""};
    let unk=||Ins::new(".long",format!("{w:#010x}"));
    match op{
        3=>Ins::new("twi",format!("{rd}, {}, {}",r(ra),fmt_simm(simm))),
        4=>paired(w,addr),
        7=>Ins::new("mulli",format!("{}, {}, {}",r(rd),r(ra),fmt_simm(simm))),
        8=>Ins::new("subfic",format!("{}, {}, {}",r(rd),r(ra),fmt_simm(simm))),
        10=>Ins::new("cmplwi",format!("{}{}, {uimm:#x}",if rd>>2!=0{format!("cr{}, ",rd>>2)}else{String::new()},r(ra))),
        11=>Ins::new("cmpwi",format!("{}{}, {}",if rd>>2!=0{format!("cr{}, ",rd>>2)}else{String::new()},r(ra),fmt_simm(simm))),
        12=>Ins::new("addic",format!("{}, {}, {}",r(rd),r(ra),fmt_simm(simm))),
        13=>Ins::new("addic.",format!("{}, {}, {}",r(rd),r(ra),fmt_simm(simm))),
        14=>if ra==0{Ins::new("li",format!("{}, {}",r(rd),fmt_simm(simm)))}else{Ins::new("addi",format!("{}, {}, {}",r(rd),r(ra),fmt_simm(simm)))},
        15=>if ra==0{Ins::new("lis",format!("{}, {}",r(rd),fmt_simm(simm)))}else{Ins::new("addis",format!("{}, {}, {}",r(rd),r(ra),fmt_simm(simm)))},
        16=>{
            let (bo,bi)=(rd,ra);let bd=((w&0xfffc) as u16 as i16) as i32;let aa=w&2!=0;let lk=w&1!=0;
            let t=if aa{bd as u32}else{addr.wrapping_add(bd as u32)};
            let mut m=match (bo&0x14,cond_name(bo,bi)){
                (0x14,_)=>"b".to_string(),
                (_,Some(c)) if bo&0x04!=0=>format!("b{}",c.split(' ').next().unwrap()),
                _=>match bo&0x1e{16|24=>"bdnz".into(),18|26=>"bdz".into(),_=>"bc".into()},
            };
            if lk{m+="l"}if aa{m+="a"}
            let pre=match cond_name(bo,bi){Some(c) if bo&0x04!=0&&c.contains(' ')=>format!("cr{}, ",bi/4),_=>String::new()};
            let ops=if m.trim_end_matches(['l','a'])=="bc"{format!("{bo}, {bi}, {t:#x}")}else{format!("{pre}{t:#x}")};
            let mut i=Ins::new(&m,ops);i.target=Some(t);i
        }
        17=>if w&2!=0{Ins::new("sc","".into())}else{unk()},
        18=>{let li=(((w&0x03fffffc)<<6) as i32)>>6;let aa=w&2!=0;let t=if aa{li as u32}else{addr.wrapping_add(li as u32)};
            let m=format!("b{}{}",if w&1!=0{"l"}else{""},if aa{"a"}else{""});let mut i=Ins::new(&m,format!("{t:#x}"));i.target=Some(t);i}
        19=>{
            let xo=(w>>1)&0x3ff;
            match xo{
                0=>Ins::new("mcrf",format!("cr{}, cr{}",rd>>2,ra>>2)),
                16|528=>{
                    let (bo,bi)=(rd,ra);let lk=w&1!=0;let base=if xo==16{"lr"}else{"ctr"};
                    let m=if bo&0x14==0x14{format!("b{base}")}else if let (Some(c),true)=(cond_name(bo,bi),bo&0x04!=0){let c=c.split(' ').next().unwrap().to_string();format!("b{c}{base}")}else{match bo&0x1e{16|24=>format!("bdnz{base}"),18|26=>format!("bdz{base}"),_=>format!("bc{base}")}};
                    let generic=m==format!("bc{base}");
                    let pre=if generic{format!("{bo}, {bi}")}else if bo&0x14!=0x14&&bi/4!=0{format!("cr{}",bi/4)}else{String::new()};
                    Ins::new(&format!("{m}{}",if lk{"l"}else{""}),pre)
                }
                33=>Ins::new("crnor",format!("{rd}, {ra}, {rb}")),50=>Ins::new("rfi","".into()),129=>Ins::new("crandc",format!("{rd}, {ra}, {rb}")),
                150=>Ins::new("isync","".into()),193=>Ins::new("crxor",format!("{rd}, {ra}, {rb}")),225=>Ins::new("crnand",format!("{rd}, {ra}, {rb}")),
                257=>Ins::new("crand",format!("{rd}, {ra}, {rb}")),289=>Ins::new("creqv",format!("{rd}, {ra}, {rb}")),417=>Ins::new("crorc",format!("{rd}, {ra}, {rb}")),
                449=>Ins::new("cror",format!("{rd}, {ra}, {rb}")),
                _=>unk(),
            }
        }
        20|21|23=>{
            let (sh,mb,me)=(rb,rc,(w>>1)&31);
            let shs=if op==23{r(rb)}else{sh.to_string()};
            let name=match op{20=>"rlwimi",21=>"rlwinm",_=>"rlwnm"};
            if op==21{
                if mb==0&&me==31-sh{return Ins::new(&format!("slwi{rcs}"),format!("{}, {}, {sh}",r(ra),r(rd)))}
                if me==31&&sh!=0&&mb==32-sh{return Ins::new(&format!("srwi{rcs}"),format!("{}, {}, {mb}",r(ra),r(rd)))}
                if sh==0&&me==31{return Ins::new(&format!("clrlwi{rcs}"),format!("{}, {}, {mb}",r(ra),r(rd)))}
                if sh==0&&mb==0{return Ins::new(&format!("clrrwi{rcs}"),format!("{}, {}, {}",r(ra),r(rd),31-me))}
                if mb==0&&me==31{return Ins::new(&format!("rotlwi{rcs}"),format!("{}, {}, {sh}",r(ra),r(rd)))}
            }
            Ins::new(&format!("{name}{rcs}"),format!("{}, {}, {shs}, {mb}, {me}",r(ra),r(rd)))
        }
        24=>if w==0x60000000{Ins::new("nop","".into())}else{Ins::new("ori",format!("{}, {}, {uimm:#x}",r(ra),r(rd)))},
        25=>Ins::new("oris",format!("{}, {}, {uimm:#x}",r(ra),r(rd))),
        26=>Ins::new("xori",format!("{}, {}, {uimm:#x}",r(ra),r(rd))),
        27=>Ins::new("xoris",format!("{}, {}, {uimm:#x}",r(ra),r(rd))),
        28=>Ins::new("andi.",format!("{}, {}, {uimm:#x}",r(ra),r(rd))),
        29=>Ins::new("andis.",format!("{}, {}, {uimm:#x}",r(ra),r(rd))),
        31=>x31(w),
        32..=47=>{
            let n=["lwz","lwzu","lbz","lbzu","stw","stwu","stb","stbu","lhz","lhzu","lha","lhau","sth","sthu","lmw","stmw"][(op-32) as usize];
            Ins::new(n,format!("{}, {}",r(rd),mem(simm,ra)))
        }
        48..=55=>{
            let n=["lfs","lfsu","lfd","lfdu","stfs","stfsu","stfd","stfdu"][(op-48) as usize];
            Ins::new(n,format!("{}, {}",f(rd),mem(simm,ra)))
        }
        56|57|60|61=>{
            // psq_l / psq_lu / psq_st / psq_stu: 12-bit displacement, W bit, GQR index.
            let d=(((w&0xfff)<<20) as i32)>>20;let wbit=(w>>15)&1;let i=(w>>12)&7;
            let n=match op{56=>"psq_l",57=>"psq_lu",60=>"psq_st",_=>"psq_stu"};
            Ins::new(n,format!("{}, {}, {wbit}, qr{i}",f(rd),mem(d,ra)))
        }
        59=>{
            let xo=(w>>1)&31;
            let n=match xo{18=>"fdivs",20=>"fsubs",21=>"fadds",22=>"fsqrts",24=>"fres",25=>"fmuls",28=>"fmsubs",29=>"fmadds",30=>"fnmsubs",31=>"fnmadds",_=>return unk()};
            let ops=match xo{25=>format!("{}, {}, {}",f(rd),f(ra),f(rc)),18|20|21=>format!("{}, {}, {}",f(rd),f(ra),f(rb)),22|24=>format!("{}, {}",f(rd),f(rb)),_=>format!("{}, {}, {}, {}",f(rd),f(ra),f(rc),f(rb))};
            Ins::new(&format!("{n}{rcs}"),ops)
        }
        63=>{
            let xo5=(w>>1)&31;
            if xo5>=18{
                let n=match xo5{18=>"fdiv",20=>"fsub",21=>"fadd",22=>"fsqrt",23=>"fsel",25=>"fmul",26=>"frsqrte",28=>"fmsub",29=>"fmadd",30=>"fnmsub",31=>"fnmadd",_=>return unk()};
                let ops=match xo5{25=>format!("{}, {}, {}",f(rd),f(ra),f(rc)),18|20|21=>format!("{}, {}, {}",f(rd),f(ra),f(rb)),22|26=>format!("{}, {}",f(rd),f(rb)),_=>format!("{}, {}, {}, {}",f(rd),f(ra),f(rc),f(rb))};
                return Ins::new(&format!("{n}{rcs}"),ops)
            }
            let xo=(w>>1)&0x3ff;
            match xo{
                0=>Ins::new("fcmpu",format!("cr{}, {}, {}",rd>>2,f(ra),f(rb))),
                32=>Ins::new("fcmpo",format!("cr{}, {}, {}",rd>>2,f(ra),f(rb))),
                12=>Ins::new(&format!("frsp{rcs}"),format!("{}, {}",f(rd),f(rb))),
                14=>Ins::new(&format!("fctiw{rcs}"),format!("{}, {}",f(rd),f(rb))),
                15=>Ins::new(&format!("fctiwz{rcs}"),format!("{}, {}",f(rd),f(rb))),
                38=>Ins::new(&format!("mtfsb1{rcs}"),format!("{rd}")),
                40=>Ins::new(&format!("fneg{rcs}"),format!("{}, {}",f(rd),f(rb))),
                64=>Ins::new("mcrfs",format!("cr{}, cr{}",rd>>2,ra>>2)),
                70=>Ins::new(&format!("mtfsb0{rcs}"),format!("{rd}")),
                72=>Ins::new(&format!("fmr{rcs}"),format!("{}, {}",f(rd),f(rb))),
                134=>Ins::new(&format!("mtfsfi{rcs}"),format!("cr{}, {}",rd>>2,(w>>12)&15)),
                136=>Ins::new(&format!("fnabs{rcs}"),format!("{}, {}",f(rd),f(rb))),
                264=>Ins::new(&format!("fabs{rcs}"),format!("{}, {}",f(rd),f(rb))),
                583=>Ins::new(&format!("mffs{rcs}"),f(rd)),
                711=>Ins::new(&format!("mtfsf{rcs}"),format!("{:#x}, {}",(w>>17)&0xff,f(rb))),
                _=>unk(),
            }
        }
        _=>unk(),
    }
}

fn x31(w:u32)->Ins{
    let rd=(w>>21)&31;let ra=(w>>16)&31;let rb=(w>>11)&31;let xo=(w>>1)&0x3ff;
    let r=|n:u32|format!("r{n}");let f=|n:u32|format!("f{n}");
    let rcs=if w&1!=0{"."}else{""};let oe=w&0x400!=0;
    let x=|n:&str|Ins::new(n,format!("{}, {}, {}",r(rd),r(ra),r(rb)));
    let xs=|n:&str|Ins::new(&format!("{n}{rcs}"),format!("{}, {}, {}",r(ra),r(rd),r(rb)));
    // XO-form arithmetic (bits 22-30 with OE)
    let xo9=(w>>1)&0x1ff;
    let arith=|n:&str,two:bool|{let m=format!("{n}{}{rcs}",if oe{"o"}else{""});if two{Ins::new(&m,format!("{}, {}",r(rd),r(ra)))}else{Ins::new(&m,format!("{}, {}, {}",r(rd),r(ra),r(rb)))}};
    match xo9{
        8=>return arith("subfc",false),10=>return arith("addc",false),11=>return arith("mulhwu",false),40=>return arith("subf",false),75=>return arith("mulhw",false),
        104=>return arith("neg",true),136=>return arith("subfe",false),138=>return arith("adde",false),200=>return arith("subfze",true),202=>return arith("addze",true),
        232=>return arith("subfme",true),234=>return arith("addme",true),235=>return arith("mullw",false),266=>return arith("add",false),459=>return arith("divwu",false),491=>return arith("divw",false),
        _=>{}
    }
    match xo{
        0=>Ins::new("cmpw",format!("{}{}, {}",if rd>>2!=0{format!("cr{}, ",rd>>2)}else{String::new()},r(ra),r(rb))),
        32=>Ins::new("cmplw",format!("{}{}, {}",if rd>>2!=0{format!("cr{}, ",rd>>2)}else{String::new()},r(ra),r(rb))),
        4=>if w==0x7fe00008{Ins::new("trap","".into())}else{Ins::new("tw",format!("{rd}, {}, {}",r(ra),r(rb)))},
        19=>Ins::new("mfcr",r(rd)),20=>x("lwarx"),23=>x("lwzx"),24=>xs("slw"),26=>Ins::new(&format!("cntlzw{rcs}"),format!("{}, {}",r(ra),r(rd))),28=>xs("and"),
        54=>Ins::new("dcbst",format!("{}, {}",r(ra),r(rb))),55=>x("lwzux"),60=>xs("andc"),83=>Ins::new("mfmsr",r(rd)),86=>Ins::new("dcbf",format!("{}, {}",r(ra),r(rb))),
        87=>x("lbzx"),119=>x("lbzux"),124=>if rd==rb{Ins::new(&format!("not{rcs}"),format!("{}, {}",r(ra),r(rd)))}else{xs("nor")},
        144=>Ins::new("mtcrf",format!("{:#x}, {}",(w>>12)&0xff,r(rd))),146=>Ins::new("mtmsr",r(rd)),150=>x("stwcx."),151=>x("stwx"),183=>x("stwux"),
        210=>Ins::new("mtsr",format!("{}, {}",(w>>16)&15,r(rd))),215=>x("stbx"),242=>Ins::new("mtsrin",format!("{}, {}",r(rd),r(rb))),246=>Ins::new("dcbtst",format!("{}, {}",r(ra),r(rb))),
        247=>x("stbux"),278=>Ins::new("dcbt",format!("{}, {}",r(ra),r(rb))),279=>x("lhzx"),284=>xs("eqv"),306=>Ins::new("tlbie",r(rb)),310=>x("eciwx"),311=>x("lhzux"),316=>xs("xor"),
        339=>{let spr=((w>>11)&0x1f)<<5|((w>>16)&0x1f);match spr{1=>Ins::new("mfxer",r(rd)),8=>Ins::new("mflr",r(rd)),9=>Ins::new("mfctr",r(rd)),_=>Ins::new("mfspr",format!("{}, {}",r(rd),spr_name(spr)))}}
        343=>x("lhax"),370=>Ins::new("tlbia","".into()),371=>{let tbr=((w>>11)&0x1f)<<5|((w>>16)&0x1f);Ins::new(if tbr==269{"mftbu"}else{"mftb"},r(rd))}
        375=>x("lhaux"),407=>x("sthx"),412=>xs("orc"),438=>x("ecowx"),439=>x("sthux"),
        444=>if rd==rb{Ins::new(&format!("mr{rcs}"),format!("{}, {}",r(ra),r(rd)))}else{xs("or")},
        467=>{let spr=((w>>11)&0x1f)<<5|((w>>16)&0x1f);match spr{1=>Ins::new("mtxer",r(rd)),8=>Ins::new("mtlr",r(rd)),9=>Ins::new("mtctr",r(rd)),_=>Ins::new("mtspr",format!("{}, {}",spr_name(spr),r(rd)))}}
        470=>Ins::new("dcbi",format!("{}, {}",r(ra),r(rb))),476=>xs("nand"),512=>Ins::new("mcrxr",format!("cr{}",rd>>2)),
        533=>Ins::new("lswx",format!("{}, {}, {}",r(rd),r(ra),r(rb))),534=>x("lwbrx"),535=>Ins::new("lfsx",format!("{}, {}, {}",f(rd),r(ra),r(rb))),536=>xs("srw"),
        566=>Ins::new("tlbsync","".into()),567=>Ins::new("lfsux",format!("{}, {}, {}",f(rd),r(ra),r(rb))),595=>Ins::new("mfsr",format!("{}, {}",r(rd),(w>>16)&15)),
        597=>Ins::new("lswi",format!("{}, {}, {}",r(rd),r(ra),rb)),598=>Ins::new("sync","".into()),599=>Ins::new("lfdx",format!("{}, {}, {}",f(rd),r(ra),r(rb))),
        631=>Ins::new("lfdux",format!("{}, {}, {}",f(rd),r(ra),r(rb))),659=>Ins::new("mfsrin",format!("{}, {}",r(rd),r(rb))),661=>Ins::new("stswx",format!("{}, {}, {}",r(rd),r(ra),r(rb))),
        662=>x("stwbrx"),663=>Ins::new("stfsx",format!("{}, {}, {}",f(rd),r(ra),r(rb))),695=>Ins::new("stfsux",format!("{}, {}, {}",f(rd),r(ra),r(rb))),
        725=>Ins::new("stswi",format!("{}, {}, {}",r(rd),r(ra),rb)),727=>Ins::new("stfdx",format!("{}, {}, {}",f(rd),r(ra),r(rb))),759=>Ins::new("stfdux",format!("{}, {}, {}",f(rd),r(ra),r(rb))),
        790=>x("lhbrx"),792=>xs("sraw"),824=>Ins::new(&format!("srawi{rcs}"),format!("{}, {}, {}",r(ra),r(rd),rb)),854=>Ins::new("eieio","".into()),
        918=>x("sthbrx"),922=>Ins::new(&format!("extsh{rcs}"),format!("{}, {}",r(ra),r(rd))),954=>Ins::new(&format!("extsb{rcs}"),format!("{}, {}",r(ra),r(rd))),
        982=>Ins::new("icbi",format!("{}, {}",r(ra),r(rb))),983=>Ins::new("stfiwx",format!("{}, {}, {}",f(rd),r(ra),r(rb))),1014=>Ins::new("dcbz",format!("{}, {}",r(ra),r(rb))),
        _=>Ins::new(".long",format!("{w:#010x}")),
    }
}

/// Gekko paired-single arithmetic (primary opcode 4) and indexed quantized load/store.
fn paired(w:u32,_addr:u32)->Ins{
    let (d,a,b,c)=((w>>21)&31,(w>>16)&31,(w>>11)&31,(w>>6)&31);let rcs=if w&1!=0{"."}else{""};
    let f=|n:u32|format!("f{n}");
    let xo5=(w>>1)&31;
    let abc=|n:&str|Ins::new(&format!("{n}{rcs}"),format!("{}, {}, {}, {}",f(d),f(a),f(c),f(b)));
    match xo5{
        10=>return abc("ps_sum0"),11=>return abc("ps_sum1"),
        12=>return Ins::new(&format!("ps_muls0{rcs}"),format!("{}, {}, {}",f(d),f(a),f(c))),13=>return Ins::new(&format!("ps_muls1{rcs}"),format!("{}, {}, {}",f(d),f(a),f(c))),
        14=>return abc("ps_madds0"),15=>return abc("ps_madds1"),
        18=>return Ins::new(&format!("ps_div{rcs}"),format!("{}, {}, {}",f(d),f(a),f(b))),20=>return Ins::new(&format!("ps_sub{rcs}"),format!("{}, {}, {}",f(d),f(a),f(b))),
        21=>return Ins::new(&format!("ps_add{rcs}"),format!("{}, {}, {}",f(d),f(a),f(b))),23=>return abc("ps_sel"),
        24=>return Ins::new(&format!("ps_res{rcs}"),format!("{}, {}",f(d),f(b))),25=>return Ins::new(&format!("ps_mul{rcs}"),format!("{}, {}, {}",f(d),f(a),f(c))),
        26=>return Ins::new(&format!("ps_rsqrte{rcs}"),format!("{}, {}",f(d),f(b))),
        28=>return abc("ps_msub"),29=>return abc("ps_madd"),30=>return abc("ps_nmsub"),31=>return abc("ps_nmadd"),
        _=>{}
    }
    let xo6=(w>>1)&0x3f;
    if xo6==6||xo6==7||xo6==38||xo6==39{
        let wbit=(w>>10)&1;let i=(w>>7)&7;
        let n=match xo6{6=>"psq_lx",7=>"psq_stx",38=>"psq_lux",_=>"psq_stux"};
        return Ins::new(n,format!("{}, r{a}, r{b}, {wbit}, qr{i}",f(d)))
    }
    let xo=(w>>1)&0x3ff;
    match xo{
        0=>Ins::new("ps_cmpu0",format!("cr{}, {}, {}",d>>2,f(a),f(b))),32=>Ins::new("ps_cmpo0",format!("cr{}, {}, {}",d>>2,f(a),f(b))),
        40=>Ins::new(&format!("ps_neg{rcs}"),format!("{}, {}",f(d),f(b))),64=>Ins::new("ps_cmpu1",format!("cr{}, {}, {}",d>>2,f(a),f(b))),
        72=>Ins::new(&format!("ps_mr{rcs}"),format!("{}, {}",f(d),f(b))),96=>Ins::new("ps_cmpo1",format!("cr{}, {}, {}",d>>2,f(a),f(b))),
        136=>Ins::new(&format!("ps_nabs{rcs}"),format!("{}, {}",f(d),f(b))),264=>Ins::new(&format!("ps_abs{rcs}"),format!("{}, {}",f(d),f(b))),
        528=>Ins::new(&format!("ps_merge00{rcs}"),format!("{}, {}, {}",f(d),f(a),f(b))),560=>Ins::new(&format!("ps_merge01{rcs}"),format!("{}, {}, {}",f(d),f(a),f(b))),
        592=>Ins::new(&format!("ps_merge10{rcs}"),format!("{}, {}, {}",f(d),f(a),f(b))),624=>Ins::new(&format!("ps_merge11{rcs}"),format!("{}, {}, {}",f(d),f(a),f(b))),
        1014=>Ins::new("dcbz_l",format!("r{a}, r{b}")),
        _=>Ins::new(".long",format!("{w:#010x}")),
    }
}

/// Disassemble a code range; `names` resolves absolute branch targets and `note` adds per-address annotations.
pub fn listing(code:&[u8],base:u32,names:&dyn Fn(u32)->Option<String>,note:&dyn Fn(u32,u32)->Option<String>)->(String,usize){
    let mut out=String::new();let mut unknown=0;
    for (i,c) in code.chunks_exact(4).enumerate(){
        let a=base+4*i as u32;let w=u32::from_be_bytes(c.try_into().unwrap());
        let ins=disasm(w,a);if ins.mnemonic==".long"{unknown+=1}
        let mut line=format!("{a:08x}: {w:08x}  {}",ins.text());
        if let Some(t)=ins.target{if let Some(n)=names(t){line+=&format!(" ; {n}")}}
        if let Some(n)=note(a,w){line+=&format!(" ; {n}")}
        out+=&line;out.push('\n');
    }
    (out,unknown)
}

#[cfg(test)]
mod tests{
    use super::*;
    #[test]
    fn known_encodings(){
        for (w,a,t) in [(0x9421ffc0u32,0,"stwu r1, -0x40(r1)"),(0x7c0802a6,0,"mflr r0"),(0x4e800020,0,"blr"),(0x38600000,0,"li r3, 0x0"),(0x3c608058,0,"lis r3, -0x7fa8"),
            (0x7c7f1b78,0,"mr r31, r3"),(0x5400103a,0,"slwi r0, r0, 2"),(0x5463463e,0,"srwi r3, r3, 24"),(0x4bef6e65,0x8013e808,"bl 0x8003566c"),(0x41820008,0x8013e830,"beq 0x8013e838"),
            (0x4180ffd8,0x8013e870,"blt 0x8013e848"),(0x4e800421,0,"bctrl"),(0x7c0903a6,0,"mtctr r0"),(0xec00f828,0,"fsubs f0, f0, f31"),(0xfc00001e,0,"fctiwz f0, f0"),
            (0x60000000,0,"nop"),(0x7c63002e,0,"lwzx r3, r3, r0"),(0x540027ff,0,"rlwinm. r0, r0, 4, 31, 31"),(0x7ce33b78,0,"mr r3, r7"),(0x4200ffe4,0x80410714,"bdnz 0x804106f8"),
            (0xe3e100f8,0,"psq_l f31, 0xf8(r1), 0, qr0"),(0x7c000774,0,"extsb r0, r0")]{
            assert_eq!(disasm(w,a).text(),t,"{w:#010x}");
        }
    }
}
