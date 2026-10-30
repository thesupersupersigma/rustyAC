# SPDX-License-Identifier: GPL-3.0-or-later
# general.txt -> flow, plus a step that returns the changelog overlay's text length right after its shot
import json, subprocess, sys
P = 're/scratch/task44c/general/page'
subprocess.run([sys.executable, 'tools/web_checks/task44b/txt2flow.py', 'tools/web_checks/task44c/general.txt', P + '/flow.json', P], check=True)
f = json.load(open(P + '/flow.json'))
FRAME = "document.querySelector('#launcher iframe').contentWindow"
i = [k for k, s in enumerate(f) if s.get('shot', '').endswith('g_changelog.jpg')][0]
f.insert(i + 1, {'js': FRAME + ".eval(\"String($('.newsdisplay pre').last().text().length)\")"})
json.dump(f, open(P + '/flow.json', 'w'))
