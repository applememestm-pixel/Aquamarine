import sys, time, json, os
from selenium import webdriver
from selenium.webdriver.common.by import By
from selenium.webdriver.common.keys import Keys
from selenium.webdriver.common.options import ArgOptions
W=os.environ.get("E2E_WORK", os.path.dirname(os.path.abspath(__file__)))
APP=os.environ.get("E2E_APP", os.path.abspath(os.path.join(os.path.dirname(__file__), "../../crates/aqua-client-ui/target/release/aqua-client-ui")))
def start():
    o=ArgOptions(); o.set_capability("tauri:options",{"application":APP}); o.set_capability("browserName","wry")
    d=webdriver.Remote("http://127.0.0.1:4444",options=o); d.implicitly_wait(0); return d
def shot(d,n): d.save_screenshot(f"{W}/{n}.png")
def js(d,code,*a): return d.execute_script(code,*a)
def heartbeat(d):
    js(d,"""if(!window.__hb){window.__hb={n:0,max:0,last:performance.now()};setInterval(()=>{const t=performance.now();const g=t-__hb.last;__hb.max=Math.max(__hb.max,g);__hb.last=t;__hb.n++;},50);}""")
def hb(d, reset=False):
    r=js(d,"const r={n:__hb.n,max:Math.round(__hb.max)}; if(arguments[0]) __hb.max=0; return r;",reset); return r
def btn(d,t): return d.find_element(By.XPATH,f"//button[contains(normalize-space(),'{t}')]")
def login(d):
    time.sleep(3)
    js(d,"localStorage.setItem('aqua.lock.v1', JSON.stringify({skipped:true})); Object.keys(localStorage).filter(k=>k.startsWith('aqua.articleDraft')).forEach(k=>localStorage.removeItem(k))"); d.refresh(); time.sleep(3)
    try: btn(d,"Continue as").click(); time.sleep(3)
    except Exception as e: print("no continue", e)
    heartbeat(d)
    js(d,"""window.__aquaTestPick=[]; window.__pick=window.__aquaTestPick; window.__errs=[];
    const ce=console.error.bind(console); console.error=(...a)=>{window.__errs.push(a.map(String).join(' ').slice(0,300)); ce(...a)};
    addEventListener('error',e=>window.__errs.push('ERR '+e.message)); addEventListener('unhandledrejection',e=>window.__errs.push('REJ '+String(e.reason).slice(0,300)));""")
def watch(d,label,secs):
    worst=0
    for i in range(secs):
        time.sleep(1); r=hb(d,True); worst=max(worst,r['max'])
    print(f"{label}: worst main-thread stall over {secs}s = {worst} ms"); return worst
def run(fn):
    d=start()
    try: login(d); fn(d)
    finally:
        try: shot(d,"last")
        except Exception: pass
        d.quit()
import subprocess
def cpu(label, secs=4):
    """CPU% of the app's processes (UI + WebKit web process) over `secs`."""
    def snap():
        out=subprocess.run(["bash","-c","for p in $(pgrep -x aqua-client-ui) $(pgrep -f WebKitWebProcess) $(pgrep -f WebKitNetworkProcess) $(pgrep -f WebKitGPUProcess); do echo $p $(awk '{print $14+$15}' /proc/$p/stat 2>/dev/null) $(cat /proc/$p/comm); done"],capture_output=True,text=True).stdout
        r={}
        for l in out.splitlines():
            parts=l.split()
            if len(parts)>=3: r[parts[0]]=(int(parts[1]),parts[2])
        return r
    a=snap(); t=time.time(); time.sleep(secs); b=snap(); dt=time.time()-t
    hz=os.sysconf('SC_CLK_TCK')
    res={b[p][1]+":"+p: round((b[p][0]-a[p][0])/hz/dt*100) for p in b if p in a}
    fps=js_holder[0] and js(js_holder[0],"return new Promise(r=>{let n=0;const s=performance.now();function f(){n++; if(performance.now()-s<1000) requestAnimationFrame(f); else r(n);} requestAnimationFrame(f);})")
    print(f"CPU {label}: {res}  rAF/s={fps}")
js_holder=[None]
