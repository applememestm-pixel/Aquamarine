from e2e import *  # noqa: F403
def t(d):
    js_holder[0]=d
    cpu('chat list idle')
    d.find_element(By.XPATH,"//*[contains(text(),'Saved Messages')]").click(); time.sleep(2)
    ce=d.find_element(By.CSS_SELECTOR,".input-wrap [contenteditable]"); ce.click(); ce.send_keys("/article"); time.sleep(0.5); ce.send_keys(Keys.ENTER); time.sleep(1.5)
    d.find_element(By.CSS_SELECTOR,".title-input").send_keys("E2E статья")
    body=d.find_element(By.CSS_SELECTOR,".body-input"); body.click(); body.send_keys("Текст."); body.send_keys(Keys.ENTER)
    watch(d,"idle composer",2)
    js(d,f"window.__pick.push(['{W}/big.mp4'])"); d.find_element(By.CSS_SELECTOR,"button.tool.photos").click(); time.sleep(2)
    print("oversize:", js(d,"return (document.body.innerText.match(/[^\\n]*too big[^\\n]*/)||[''])[0]"), "| tiles:", js(d,"return document.body.innerText.includes('Attachments')"))
    js(d,f"window.__pick.push(['{W}/clip.mp4'])"); d.find_element(By.CSS_SELECTOR,"button.tool.photos").click()
    watch(d,"after adding 56MB video",8); shot(d,"s4_added")
    js(d,f"window.__pick.push(['{W}/song.mp3'])"); d.find_elements(By.CSS_SELECTOR,"button.tool.photos")[1].click()
    watch(d,"after adding music",3)
    [b for b in d.find_elements(By.XPATH,"//button[normalize-space()='Publish']") if b.is_displayed()][0].click()
    for i in range(60):
        time.sleep(1)
        st=js(d,"const t=document.body.innerText; return (t.match(/(Could not[^\\n]*|Uploading[^\\n]*|Publishing[^\\n]*|Article published|Give the[^\\n]*|Write something[^\\n]*)/g)||[]).join(' | ')")
        if i<5 or i%5==0: print(" t+%d"%(i+1), st)
        if not js(d,"return !!document.querySelector('.title-input')"): print("composer closed after",i+1,"s"); break
    print("hb during publish", hb(d,True))
    watch(d,"after composer closed",5); cpu("after publish (card visible)"); shot(d,"s4_published")
    print("errors:", js(d,"return window.__errs"))
    # --- play the track from the card
    print("toasts:", js(d,"return [...document.querySelectorAll('.toast, [class*=toast]')].map(t=>t.innerText).filter(Boolean).slice(0,3)"))
    trk=[b for b in d.find_elements(By.CSS_SELECTOR,".music .trk") if b.is_displayed()]
    print("tracks in card:", len(trk))
    if trk:
        trk[0].click(); watch(d,"after clicking the track",4); cpu("track clicked"); time.sleep(2); print("audio t after 2s:", js(d,"const a=document.querySelector('.top-player audio'); return a&&[a.currentTime,a.paused]"))
        print("top audio:", js(d,"const a=document.querySelector('.top-player audio'); return a? {src:a.src, t:a.currentTime, paused:a.paused, rs:a.readyState, err:a.error&&a.error.code}:null"))
    shot(d,"s4_music")
    # --- open the reader and close it
    b=[x for x in d.find_elements(By.XPATH,"//button[contains(.,'Read article') or contains(.,'READ ARTICLE')]") if x.is_displayed()]
    print("read buttons", len(b))
    if b:
        b[-1].click(); watch(d,"reader open",3); cpu("reader open"); shot(d,"s4_reader")
        d.find_element(By.TAG_NAME,"body").send_keys(Keys.ESCAPE); watch(d,"after closing reader",3); cpu("reader closed"); shot(d,"s4_closed")
    print("top audio:", js(d,"const a=document.querySelector('.top-player audio'); return a? {src:a.src, t:a.currentTime, paused:a.paused, rs:a.readyState, err:a.error&&a.error.code}:null"))
    print("errors:", js(d,"return window.__errs"))
run(t)
