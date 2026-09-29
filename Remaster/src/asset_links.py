"""Local dependency scopes for loose and nested archive assets."""
from pathlib import Path

def sibling_sources(source,suffix):
    import research
    if '::' not in source:
        return [str(p) for p in sorted(Path(source).parent.iterdir()) if p.is_file() and p.suffix.lower()==suffix]
    parent,name=source.rsplit('::',1);data,_=research.read_virtual(parent)
    folder=name.replace('\\','/').rsplit('/',1)[0] if '/' in name.replace('\\','/') else ''
    return [parent+'::'+e['name'] for e in research.entries(data) or [] if Path(e['name']).suffix.lower()==suffix and (e['name'].replace('\\','/').rsplit('/',1)[0] if '/' in e['name'].replace('\\','/') else '')==folder]

def texture_sources(source):
    # Archives can reference banks beside their containing archive (world.big
    # does). Walk only these containment scopes, never unrelated global banks.
    result=[];scope=str(source)
    while True:
        result.extend(sibling_sources(scope,'.gsh'))
        if '::' not in scope:break
        scope=scope.rsplit('::',1)[0]
    return list(dict.fromkeys(result))
