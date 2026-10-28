# Task 43b step 2c: the page joins a server through AC's launcher (the theme's Drive -> Online,
# the server, the car, Join), drives the race to its end, leaves, and AC's launcher shows the
# results page from race_out.json. The theme's own buttons are clicked inside its frame.
# usage: mkflow.py <out.json> <run dir> <car> [explore]
import json, sys
out, run, car = sys.argv[1], sys.argv[2], sys.argv[3]
explore = len(sys.argv) > 4
# (the theme's jQuery, in its frame; `poll(sel)` waits for a visible match)
pre = ("(async()=>{const w=document.querySelector('#launcher iframe').contentWindow;const $=w.$;"
       "const poll=(sel,s=30)=>new Promise((ok,no)=>{const t0=Date.now();const t=setInterval(()=>{const m=$(sel);if(m.length){clearInterval(t);ok(m)}else if(Date.now()-t0>s*1000){clearInterval(t);no(new Error('no '+sel))}},200)});")
def theme(body):
    return {"js": pre + body + "})()"}
# (the multiplayer agreement and the warning come first: their OK, as a player clicks it)
online = theme("$('#drive .tabs div[data-mode=online]').click();const t0=Date.now();"
               "while(!$('.online .list .item').length){$('.dialog:visible .options .ok:visible').click();await new Promise((ok)=>setTimeout(ok,500));if(Date.now()-t0>30000)throw new Error('no server list')}"
               "return $('.online .list .item').map((i,e)=>$(e).text().replace(/\\s+/g,' ').trim()).get().join(' | ')")
# (the address bar's server: the site's own servers are real ones, never clicked)
pick = theme(f"$('.online .list .item').filter((i,e)=>$(e).text().includes(\"the address bar's server\")).click();await poll('.serverdetails .carlist .value > div');"
             f"const c=$('.serverdetails .carlist .value > div').filter((i,e)=>$(e).data('id')=={json.dumps(car)});c.click();"
             "return 'cars: '+$('.serverdetails .carlist .value > div').map((i,e)=>$(e).data('id')).get().join(',')+' picked '+c.length+' join buttons '+$('[data-mode=join]').length")
join = theme("$('[data-mode=join]:visible').first().click();return 'join clicked'")
dump = {"js": "JSON.stringify({state:window.rustyac.state,launcher:window.rustyac.launcher})"}
hud = ("JSON.stringify((({kmh,gear,in_pit_lane,seconds,position})=>({kmh,gear,in_pit_lane,seconds,position}))(window.rustyac.hud||{}))"
       "+' log='+JSON.stringify(window.rustyac.log.filter((l)=>l.startsWith('online: the')||l.startsWith('online: a new')||l.startsWith('referee')))")
def wait_log(text):
    return {"js": f"new Promise((done)=>{{const t=setInterval(()=>{{if(window.rustyac.log.some((l)=>l.includes({json.dumps(text)}))){{clearInterval(t);done({hud})}}}},100)}})"}
# the theme's results page: its text, after ACStoppedEvent
# (the theme answers ACStoppedEvent with UIResults.Refresh: the launcher's getresults, that is
# the page's race_out.json; the frame may still be loading)
results = {"js": "(async()=>{let w,t0=Date.now();while(Date.now()-t0<60000){w=(document.querySelector('#launcher iframe')||{}).contentWindow;if(w&&w.UIResults&&w.UIResults.resultData)break;await new Promise((ok)=>setTimeout(ok,500))}"
           "await new Promise((ok)=>setTimeout(ok,3000));const r=w.$.ACCall('getresults');const d=r&&r.resultData;"
           "return JSON.stringify({shown:!!(w.UIResults&&w.UIResults.resultData),track:d&&d.track,players:d&&d.players.slice(0,3),"
           "sessions:d&&d.sessions.map((s)=>({name:s.name,type:s.type,laps:s.laps,bestLaps:s.bestLaps,raceResult:s.raceResult.slice(0,3)}))})})()"}
flow = [{"until": "launcher"}, {"wait": 3}, online, {"shot": f"{run}/b_online.png"}, pick, {"wait": 1}, {"shot": f"{run}/b_picked.png"}]
if explore:
    flow += [dump]
else:
    flow += [join, {"until": "driving"}, {"wait": 5}, {"js": hud}, {"shot": f"{run}/b_drive.png"},
             wait_log("the results came"), {"wait": 3}, {"shot": f"{run}/b_results_in_game.png"},
             {"js": "window.rustyac.leave(); 'left'"}, {"until": "launcher"}, results, {"shot": f"{run}/b_launcher_best.png"},
             # (the "new personal best" box first: its continue, then the results page)
             theme("$('.dialog:visible .options .ok:visible').click();await new Promise((ok)=>setTimeout(ok,3000));"
                   "return $('.results:visible, .result:visible').first().text().replace(/\\s+/g,' ').trim().slice(0,1200)"),
             {"shot": f"{run}/b_launcher_results.png"}, dump]
json.dump(flow, open(out, 'w'), indent=1)
