# SPDX-License-Identifier: GPL-3.0-or-later
# settings.txt (rustyac-launcher --script steps) -> a check.mjs --flow, each step run in the theme's frame
#   python txt2flow.py <script.txt> <out.json> <shot folder>
# At `log MARK` and at the end: the request list and the player's folder (snapshot) as values.
import json
import sys

FRAME = "document.querySelector('#launcher iframe').contentWindow"
# the player's folder as the page keeps it: {folders, files: {path: one char per byte}}
SNAP = {'js': r'''(()=>{const s=window.rustyacTheme.launcher.snapshot();const out={folders:[],files:{}};let at=0;
const bytes=(b)=>Array.from(b,(c)=>String.fromCharCode(c)).join('');
while(at<s.length){const nl=s.indexOf(10,at);const line=new TextDecoder().decode(s.subarray(at,nl));at=nl+1;
if(line.startsWith('D ')){out.folders.push(line.slice(2));continue}
const [path,ms,len]=line.slice(2).split('\t');out.files[path]=bytes(s.subarray(at,at+Number(len)));at+=Number(len)}
return JSON.stringify(out)})()'''}

steps = [{'until': 'launcher'}]
for line in open(sys.argv[1], encoding='utf-8').read().splitlines():
    if not line or line.startswith('#'):
        continue
    op, _, rest = line.partition(' ')
    if op == 'wait':
        steps.append({'wait': int(rest) / 1000})
    elif op == 'click':
        code = f"var e=$({json.dumps(rest)});if(!e.length)throw 'nothing matches {rest}';e.first().click();"
        steps.append({'js': f"{FRAME}.eval({json.dumps(code)}),1"})
    elif op == 'js':
        steps.append({'js': f"{FRAME}.eval({json.dumps(rest)}),1"})
    elif op == 'until':
        ms, _, expr = rest.partition(' ')
        steps.append({'js': "new Promise((ok,no)=>{const t0=Date.now();const f=()=>{try{if(%s.eval(%s))return ok(1)}catch(e){}if(Date.now()-t0>%s)return no('until timed out');setTimeout(f,250)};f()})" % (FRAME, json.dumps(expr), ms)})
    elif op == 'shot':
        steps.append({'shot': f"{sys.argv[3]}/{rest.split('/')[-1].replace('.png', '.jpg')}"})
    elif op == 'log':
        steps.append({'js': f"window.rustyacTheme.requests.push('LOG {rest}'),1"})
        if rest == 'MARK':
            steps.append(SNAP)
    else:
        sys.exit(f'unknown step {line}')
steps.append({'js': 'JSON.stringify(window.rustyacTheme.requests)'})
steps.append(SNAP)
json.dump(steps, open(sys.argv[2], 'w'), indent=1)
