# SPDX-License-Identifier: GPL-3.0-or-later
# Task 44b: the rustyAC tab's options in the page, the preset's way back into AC's Video tab, the
# Controls tab, Close, then a drive and its overlay.  python mktab.py <out.json> <shot folder>
import json
import sys

out, shots = sys.argv[1], sys.argv[2]
F = "document.querySelector('#launcher iframe').contentWindow"
fr = lambda code: {'js': f'{F}.eval({json.dumps(code)})'}
top = lambda code: {'js': code}
until = lambda expr, ms=120000: {'js': "new Promise((ok,no)=>{const t0=Date.now();const f=()=>{try{const v=(%s);if(v)return ok(v)}catch(e){}if(Date.now()-t0>%d)return no('until timed out: '+%s);setTimeout(f,250)};f()})" % (expr, ms, json.dumps(expr))}
ready = f"{F}.jQuery&&!{F}.$('body').hasClass('loading')&&{F}.$('#menu li[data-panel=options]').length>0"
# a value into an input of the page, as a person's change gives it
put = lambda q, v, ev='change': top(f"(()=>{{const e=document.querySelector({json.dumps(q)});e.value={json.dumps(v)};e.dispatchEvent(new Event('input'));e.dispatchEvent(new Event({json.dumps(ev)}));return e.value}})()")
row = lambda label: f"[...document.querySelectorAll('#controls-list .control')].find((c)=>c.querySelector('.control-row span').textContent==={json.dumps(label)})"
state = top("JSON.stringify({sound:localStorage.getItem('rustyac.sound'),picture:localStorage.getItem('rustyac.picture'),scale:localStorage.getItem('rustyac.renderScale'),"
            "controls:(JSON.parse(localStorage.getItem('rustyac.controls')||'{}').text||'').split(/\\r?\\n/).filter((l)=>/SPEED_SENSITIVITY|STEERING_SPEED/.test(l)),"
            "video:new TextDecoder().decode(window.rustyacTheme.launcher.read_doc('cfg\\\\video.ini')).split(/\\r?\\n/).filter((l)=>/SHADOW_MAP_SIZE|WORLD_DETAIL|ANISOTROPIC|WIDTH/.test(l))})")
panel = top("JSON.stringify({hidden:document.getElementById('controls').hidden,docked:document.getElementById('controls').className,rel:" + F + ".document.body.getAttribute('rel'),panel:" + F + ".document.body.getAttribute('data-panel')})")
flow = [
    {'until': 'launcher'}, until(f"{F}.$&&{F}.$('div.flag[data-mode=en]').length"), {'wait': 2}, fr("$('div.flag[data-mode=en]').click(),1"), {'wait': 8}, until(ready), {'wait': 2},
    # the corner: no gear, no full screen; Settings and Drive the only tiles
    top("JSON.stringify({gear:!document.getElementById('gear').hidden,corner:[...document.querySelectorAll('#corner button')].filter((b)=>!b.hidden).map((b)=>b.id),"
        "tiles:[..." + F + ".document.querySelectorAll('#menu li[data-panel]')].filter((t)=>t.offsetParent&&getComputedStyle(t).display!=='none').map((t)=>t.dataset.panel)})"),
    fr("$('#menu li[data-panel=career]').click(),1"), {'wait': 1}, top(f"{F}.document.body.getAttribute('data-panel')"),
    fr("$('#menu li[data-panel=options]').click(),1"), {'wait': 2},
    fr("$('#benchmark').click(),1"), {'wait': 1}, top("JSON.stringify({notice:window.rustyac.launcher.notice,starts:window.rustyac.launcher.starts})"), {'shot': f'{shots}/benchmark.jpg'}, fr("$('.notice .button, .notice input[type=button]').first().click(),1"), {'wait': 1},
    fr("document.querySelector('#options > .tabs > div[data-mode=rustyac]').click(),1"), {'wait': 1}, panel, {'shot': f'{shots}/tab_rustyac.jpg'},
    state,
    put('#sound-volume', '40'), top("document.getElementById('sound-mute').click(),document.getElementById('sound-mute').checked"),
    put('#backend', 'auto'),
    put('#gfx-list input[type=range]', '80'),
    top(f"(()=>{{const r={row('Pad: speed sensitivity')};const e=r.querySelector('input[type=number]');e.value='0.3';e.dispatchEvent(new Event('change'));return e.value}})()"),
    top(f"(()=>{{const r={row('Keys: steering speed')};const e=r.querySelector('input[type=number]');e.value='1.5';e.dispatchEvent(new Event('change'));return e.value}})()"),
    state,
    top("JSON.stringify({storage:document.querySelectorAll('#storage-list tr').length,total:document.getElementById('storage-total').textContent,keys:document.querySelectorAll('#controls table.small tr').length})"),
    top("document.getElementById('copy-diagnostics').click(),1"), {'wait': 1}, top("document.getElementById('diagnostics-copied').textContent+' | '+(window.rustyac.diagnostics||'').split('\\n').length"),
    {'cdp': 'Runtime.evaluate', 'params': {'expression': "document.getElementById('fullscreen').click()", 'userGesture': True}}, {'wait': 1},
    top("String(!!document.fullscreenElement)"), top("document.fullscreenElement?document.exitFullscreen().then(()=>'left'):'none'"), {'wait': 1},
    # the preset: video.ini, then the theme reads it again with the rustyAC tab shown again
    put('#gfx-preset', 'low'), {'wait': 2}, until(f"{F}.document.body.getAttribute('rel')==='rustyac'&&!document.getElementById('controls').hidden", 60000), {'wait': 2}, panel, state,
    top("document.getElementById('gfx-preset').value"),
    fr("document.querySelector('#options > .tabs > div[data-mode=video]').click(),1"), {'wait': 1},
    fr("JSON.stringify({shadow:$('#shadowmap').slider('value'),af:$('#af').slider('value')})"), panel,
    # AC's Controls tab: the rustyAC tab at its Controls
    fr("document.querySelector('#options > .tabs > div[data-mode=controls]').click(),1"), {'wait': 1}, panel,
    top("Math.round(document.getElementById('controls-section').getBoundingClientRect().top-document.getElementById('controls').getBoundingClientRect().top)"),
    {'shot': f'{shots}/tab_controls.jpg'},
    # Close: Settings' Video tab
    top("document.getElementById('controls-close').click(),1"), {'wait': 1}, panel,
    # a drive: the gear, its overlay (not docked), O and Esc
    fr("$('#menu li[data-panel=drive]').click(),1"), {'wait': 2}, until(f"{F}.$('.drive .tabs > div[data-mode=practice]').length"),
    fr("try{UI.checkVideomode=false}catch(e){};$('.drive .tabs > div[data-mode=practice]').click(),1"), {'wait': 2},
    fr("$('.drive .submenu .button[data-mode=\"brands,cars\"]').click(),1"), {'wait': 2}, until(f"{F}.$('#drive .cars .item[data-car=bmw_z4_gt3]').length"),
    fr("$('#drive .cars .item[data-car=bmw_z4_gt3]').click(),1"), {'wait': 2},
    fr("$('.drive .submenu .button[data-mode=tracks]').click(),1"), {'wait': 2}, until(f"{F}.$('#drive .tracks .item[data-track=ks_laguna_seca]').length"),
    fr("$('#drive .tracks .item[data-track=ks_laguna_seca]').click(),1"), {'wait': 2},
    fr("$('.drive .submenu .button[data-mode=summary]').click(),1"), {'wait': 2},
    fr("$('.drive .submenu .button[data-mode=startrace], .summary .nav .button[data-mode=startrace]').not('.inactive,.disabled').first().click(),1"),
    {'until': 'driving'}, {'wait': 6},
    top("JSON.stringify({gear:!document.getElementById('gear').hidden,controls:document.getElementById('controls').hidden})"),
    {'shot': f'{shots}/drive_gear.jpg'},
    top("document.getElementById('gear').click(),1"), {'wait': 1}, panel, top("document.activeElement.id||document.activeElement.tagName"),
    {'shot': f'{shots}/drive_overlay.jpg'},
    {'cdp': 'Input.dispatchKeyEvent', 'params': {'type': 'rawKeyDown', 'code': 'Escape', 'key': 'Escape', 'windowsVirtualKeyCode': 27}},
    {'cdp': 'Input.dispatchKeyEvent', 'params': {'type': 'keyUp', 'code': 'Escape', 'key': 'Escape', 'windowsVirtualKeyCode': 27}}, {'wait': 1},
    top("JSON.stringify({state:window.rustyac.state,controls:document.getElementById('controls').hidden})"),
    {'cdp': 'Input.dispatchKeyEvent', 'params': {'type': 'rawKeyDown', 'code': 'KeyO', 'key': 'o', 'windowsVirtualKeyCode': 79}},
    {'cdp': 'Input.dispatchKeyEvent', 'params': {'type': 'keyUp', 'code': 'KeyO', 'key': 'o', 'windowsVirtualKeyCode': 79}}, {'wait': 1},
    top("JSON.stringify({state:window.rustyac.state,controls:document.getElementById('controls').hidden,docked:document.getElementById('controls').className})"),
    top("JSON.stringify({starts:window.rustyac.launcher.starts,errors:window.rustyac.launcher.errors})"),
]
json.dump(flow, open(out, 'w'), indent=1)
