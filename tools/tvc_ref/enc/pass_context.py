"""Research-only source conversion: thread an explicit picture context through calls.

The converter fails closed on missing expected declarations. Generated output is
validated against the serial encoder; this is not a general-purpose C parser.
"""
import re

def convert_context(source):
    old='static _Thread_local EncPictureContext *enc_ctx;'
    if source.count(old)!=1: raise RuntimeError('context declaration drift')
    source=source.replace(old,'/* Context is passed explicitly to picture helpers. */')
    old='BJob *j=arg;enc_ctx=&j->context;'
    if source.count(old)!=1: raise RuntimeError('worker context binding drift')
    source=source.replace(old,'BJob *j=arg;EncPictureContext *enc_ctx=&j->context;')
    old='EncPictureContext main_context = {0}; enc_ctx = &main_context;'
    if source.count(old)!=1: raise RuntimeError('main context binding drift')
    source=source.replace(old,'EncPictureContext main_context = {0}; EncPictureContext *enc_ctx = &main_context;')
    def hide(m): return ''.join('\n' if c=='\n' else ' ' for c in m.group())
    mask=re.sub(r'/\*.*?\*/|//[^\n]*|"(?:\\.|[^"\\])*"|\'(?:\\.|[^\'\\])*\'',hide,source,flags=re.S)
    pattern=re.compile(r'(?m)^static\s+(?:[\w*]+\s+|\*\s*)+([A-Za-z_]\w*)\s*\(([^;{}]*)\)\s*\{')
    defs=[]
    for m in pattern.finditer(mask):
        start=m.end();depth=1;pos=start
        while depth and pos<len(mask):
            if mask[pos]=='{':depth+=1
            elif mask[pos]=='}':depth-=1
            pos+=1
        if depth:raise RuntimeError('unbalanced function '+m.group(1))
        defs.append((m.group(1),m,start,pos-1))
    names={n for n,_,_,_ in defs}; boundaries={'bwork','bpool_work'}
    needed={n for n,_,a,b in defs if n not in boundaries and re.search(r'\b(?:S|SRC|CUR|REFS|enc_ctx)\b',mask[a:b])}
    changed=True
    while changed:
        changed=False
        for n,_,a,b in defs:
            if n in needed or n in boundaries:continue
            if any(re.search(r'\b'+re.escape(target)+r'\s*\(',mask[a:b]) for target in needed):needed.add(n);changed=True
    main=re.search(r'(?m)^int main\s*\([^{}]*\)\s*\{',mask)
    if not main or not {'encode_picture_buf','motion_search','tree_offsets'} <= needed:raise RuntimeError('unexpected encoder call graph')
    bodies=[(a,b) for _,_,a,b in defs]+[(main.end(),len(mask))]
    edits=[]
    for n,m,_,_ in defs:
        if n not in needed:continue
        open_paren=mask.index('(',m.start(),m.end());arg_start=open_paren+1
        if mask[arg_start:m.end()].lstrip().startswith('void)'):
            v=mask.index('void',arg_start,m.end());edits.append((v,v+4,'EncPictureContext *enc_ctx'))
        else:edits.append((arg_start,arg_start,'EncPictureContext *enc_ctx, '))
    call_pattern=re.compile(r'\b('+'|'.join(map(re.escape,sorted(needed,key=len,reverse=True)))+r')\s*\(')
    for a,b in bodies:
        for m in call_pattern.finditer(mask,a,b):
            at=m.end();empty=mask[at:].lstrip().startswith(')')
            edits.append((at,at,'enc_ctx' if empty else 'enc_ctx, '))
    for a,b,replacement in sorted(edits,reverse=True):source=source[:a]+replacement+source[b:]
    return source, sorted(needed)
