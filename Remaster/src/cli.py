"""Console entry point. JSON stdout, diagnostics stderr, no GUI imports."""
import argparse
import json
import re
import sys
from pathlib import Path
import research


def main(argv=None):
    p=argparse.ArgumentParser(description='Headless EAGL format research; no GUI required.')
    sub=p.add_subparsers(dest='command',required=True)
    for name in ('inspect','inventory','validate','decode','extract','hexdump','strings'):
        cmd=sub.add_parser(name)
        cmd.add_argument('source',help='File/directory, or outer.big::member.viv::file.o')
        if name in ('inspect','inventory','validate'):
            cmd.add_argument('--out',type=Path,help='Write JSON report to this new path')
            cmd.add_argument('--deep',action='store_true',help='Run recovered payload decoders and index checks')
        if name in ('decode','extract'):cmd.add_argument('--out',type=Path,required=True,help='Output directory; existing files are not overwritten')
        if name=='decode':
            cmd.add_argument('--skeleton');cmd.add_argument('--clip',type=int)
        if name=='hexdump':
            cmd.add_argument('--offset',type=lambda s:int(s,0),default=0);cmd.add_argument('--length',type=lambda s:int(s,0),default=256)
        if name=='strings':cmd.add_argument('--minimum',type=int,default=5)
    args=p.parse_args(argv)
    try:
        if args.command=='inspect':result=research.inspect(args.source,args.deep)
        elif args.command in ('inventory','validate'):
            result=research.catalog(args.source,deep=args.deep,progress=lambda s:print(s,file=sys.stderr))
        elif args.command=='decode':result=research.decode(args.source,args.out,args.skeleton,args.clip)
        elif args.command=='extract':
            from archives import safe_target,decompress
            import core
            data,name=research.read_virtual(args.source);table=research.entries(data)
            if table is None:raise ValueError('Only BIGF/BIG4/VIV and Nintendo U8 extraction implemented')
            targets=[safe_target(args.out,e['name']) for e in table]
            if len({str(t).casefold() for t in targets})!=len(targets) or any(t.exists() for t in targets):raise ValueError('Duplicate or existing extraction output paths')
            for e,t in zip(table,targets):core.write_new(t,decompress(data[e['offset']:e['offset']+e['size']]))
            result=dict(extracted=len(table),output=str(args.out.resolve()))
        elif args.command=='hexdump':
            data,name=research.read_virtual(args.source)
            from containers import span
            block=span(data,args.offset,args.length)
            result=dict(source=args.source,lines=[dict(offset=args.offset+i,hex=block[i:i+16].hex(' '),ascii=''.join(chr(c) if 32<=c<127 else '.' for c in block[i:i+16])) for i in range(0,len(block),16)])
        else:
            if args.minimum<1:raise ValueError('--minimum must be positive')
            data,name=research.read_virtual(args.source)
            result=dict(source=args.source,strings=[dict(offset=m.start(),text=m.group().decode('ascii')) for m in re.finditer(rb'[\x20-\x7e]{'+str(args.minimum).encode()+rb',}',data)])
        text=json.dumps(result,ensure_ascii=False,indent=2,default=research.json_default,allow_nan=False)+'\n'
        if args.command in ('inspect','inventory','validate') and args.out:
            args.out.parent.mkdir(parents=True,exist_ok=True)
            with args.out.open('x',encoding='utf-8') as f:f.write(text)
            if args.command in ('inventory','validate'):
                with args.out.with_suffix('.md').open('x',encoding='utf-8') as f:f.write(research.coverage_markdown(result))
            print(json.dumps(dict(report=str(args.out),summary=result.get('summary'))))
        else:print(text,end='')
        if args.command=='validate':
            failed=bool(result['errors']) or any(r['status']=='error' or r.get('payload',{}).get('status')=='unsupported_or_invalid' or any(i.get('status')=='error' for i in r.get('payload',{}).get('images',[])) for r in result['records'])
            return 2 if failed else 0
        return 0
    except Exception as exc:
        print(json.dumps(dict(status='error',error=str(exc)),ensure_ascii=False),file=sys.stderr)
        return 1


if __name__=='__main__':
    if hasattr(sys.stdout,'reconfigure'):sys.stdout.reconfigure(encoding='utf-8')
    if hasattr(sys.stderr,'reconfigure'):sys.stderr.reconfigure(encoding='utf-8')
    raise SystemExit(main())
