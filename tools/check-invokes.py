"""Compare every invoke("cmd", {args}) in the UI against the Rust #[tauri::command] params."""
# Run from crates/aqua-client-ui:  python3 ../../tools/check-invokes.py
import re,glob,sys
rs=""
for f in glob.glob("src/**/*.rs",recursive=True): rs+=open(f).read()
cmds={}
for m in re.finditer(r'#\[tauri::command[^\]]*\]\s*(?:pub\s+)?(?:async\s+)?fn\s+(\w+)\s*(?:<[^>]*>)?\s*\((.*?)\)\s*(?:->|\{)',rs,re.S):
    name,params=m.group(1),m.group(2)
    ps=[]
    for p in re.split(r',(?![^<]*>)',params):
        p=p.strip()
        if not p or ':' not in p: continue
        n,ty=p.split(':',1); n=re.sub(r'^mut\s+','',n.strip())
        if re.search(r'State<|AppHandle|Window\b|WebviewWindow|Webview\b',ty): continue
        ps.append((re.sub(r'_(\w)',lambda x:x.group(1).upper(),n),'Option<' in ty))
    cmds[name]=ps
ts=""
for f in glob.glob("ui/src/**/*.ts",recursive=True)+glob.glob("ui/src/**/*.svelte",recursive=True): ts+=open(f).read()+"\n"
bad=0
for m in re.finditer(r'invoke(?:<[^>]*>)?\(\s*"(\w+)"\s*(?:,\s*\{([^}]*)\})?',ts):
    c,args=m.group(1),m.group(2) or ""
    if c not in cmds:
        print("UNKNOWN command",c); bad+=1; continue
    keys=set(); spread=False
    for part in args.split(','):
        part=part.strip()
        if not part: continue
        if part.startswith('...'): spread=True; continue
        keys.add(part.split(':')[0].strip())
    if spread: continue
    for p,opt in cmds[c]:
        if p not in keys and not opt: print("MISSING",c,p,"given",sorted(keys)); bad+=1
    for k in keys - {p for p,_ in cmds[c]}: print("EXTRA (ignored by Tauri)",c,k)
print("problems:",bad); sys.exit(1 if bad else 0)
