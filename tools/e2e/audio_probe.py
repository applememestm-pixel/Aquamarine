"""Where does the first-play stall come from: WebKit/GStreamer or our UI?"""
from e2e import *  # noqa: F403
def go(d):
    js_holder[0]=d
    watch(d,"idle",2)
    url=d.execute_async_script("""const cb=arguments[arguments.length-1];
      window.__TAURI_INTERNALS__.invoke('media_stream_url',{path:arguments[0]}).then(cb,e=>cb('ERR '+e));""", W+"/song.mp3")
    print("url", url)
    for i in range(3):
        hb(d,True)
        js(d,"window.__a=new Audio(arguments[0]); window.__a.play().catch(e=>window.__errs.push(String(e)));",url)
        watch(d,f"bare Audio play #{i+1}",3)
        js(d,"window.__a.pause(); window.__a.src=''; window.__a=null")
        time.sleep(1)
    print("errors:", js(d,"return window.__errs"))
run(go)
