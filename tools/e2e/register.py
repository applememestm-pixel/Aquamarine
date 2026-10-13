"""Create the test account (dev-mode codes) unless this data dir has one."""
from e2e import *  # noqa: F403
d = start(); time.sleep(4)
try:
    if not d.find_elements(By.XPATH, "//button[contains(normalize-space(),'Continue as')]"):
        def inp(ph): return [i for i in d.find_elements(By.CSS_SELECTOR, f"input[placeholder='{ph}']") if i.is_displayed()][0]
        srv = inp("http://127.0.0.1:7777"); srv.clear(); srv.send_keys("http://127.0.0.1:7777")
        inp("you@example.com").send_keys("e2e@test.local")
        for p in [i for i in d.find_elements(By.CSS_SELECTOR, "input[type=password]") if i.is_displayed()]: p.send_keys("Passw0rd!123")
        btn(d, "Send code").click(); time.sleep(3)
        btn(d, "Verify").click(); time.sleep(3)
        inp("ivan2027").send_keys("e2etester"); inp("Иван Иванов").send_keys("E2E Tester")
        btn(d, "Finish").click(); time.sleep(4)
        print("registered @e2etester")
finally:
    d.quit()
