# Headless perf probe: reads the ?perf=1 overlay while idle and while panning.
import sys, time
from playwright.sync_api import sync_playwright
url = sys.argv[1] if len(sys.argv) > 1 else "http://localhost:1420/?perf=1"
shot = sys.argv[2] if len(sys.argv) > 2 else None
with sync_playwright() as p:
    b = p.chromium.launch(channel="chrome", headless=True)
    pg = b.new_page(viewport={"width": 1600, "height": 1000})
    pg.goto(url)
    pg.wait_for_selector(".perf-overlay", timeout=20000)
    time.sleep(6)
    def sample(label, n=4):
        for _ in range(n):
            time.sleep(1)
            print(label, pg.inner_text(".perf-overlay").replace("\n", " | "))
    sample("idle")
    pg.mouse.move(800, 500)
    pg.mouse.down()
    t0 = time.time()
    x = 800
    while time.time() - t0 < 3:
        x = 700 if x > 800 else 900
        pg.mouse.move(x, 500, steps=8)
    pg.mouse.up()
    print("pan  ", pg.inner_text(".perf-overlay").replace("\n", " | "))
    print("dom nodes", pg.evaluate("document.querySelectorAll('svg *').length"))
    if shot:
        pg.screenshot(path=shot)
    b.close()
