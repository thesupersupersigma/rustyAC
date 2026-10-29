# SPDX-License-Identifier: GPL-3.0-or-later
# The Settings session's requests, desktop (run.log via tools/proofs/log2req.py) vs page
# (rustyacTheme.requests), from `log MARK` on, cache-busting times blanked.
#   python cmp.py <run.log> <page_requests.txt>
import difflib
import os
import re
import subprocess
import sys

log = open(sys.argv[1], encoding='utf-8', errors='replace').read().split('\n')
cut = os.path.join(os.path.dirname(sys.argv[1]), 'after_mark.log')
open(cut, 'w', encoding='utf-8').write('\n'.join(log[next(i for i, l in enumerate(log) if l.strip() == 'log MARK'):]))
req = cut + '.req'
subprocess.run([sys.executable, 'tools/proofs/log2req.py', cut, req], check=True, capture_output=True)

DROP = {'start', 'getguid', 'getachievements', 'getserver', 'getserverlist', 'getservercached', 'getlanservers', 'pingserver'}
# the theme's Special Events poll (a timer, not a click; desktop's off-screen window logs none)
TIMER = {'eventcount'}
host = lambda l: re.match(r'\S+ ac://([^/?]*)', l).group(1)


def norm(l):
    # (WebView2 writes a bare host with a slash: ac://showcontrols/?123)
    l = re.sub(r'^(\S+ ac://[^/?\s]+)\?', r'\1/?', l)
    return re.sub(r'([?&]_=)\d+|(?<=/\?)\d{13}$', lambda m: (m.group(1) or '') + 'T', l)


def until(ls):
    # desktop's Controls page (rustyAC's own, after showcontrols) reads controls.ini through the
    # bridge; the page's rustyAC tab has it already
    return ls[: next((i + 1 for i, l in enumerate(ls) if 'ac://showcontrols' in l), len(ls))]


desk = until([norm(l) for l in open(req, encoding='utf-8').read().splitlines()[1:] if host(l) not in TIMER])
page = open(sys.argv[2], encoding='utf-8').read().splitlines()
page = page[page.index('LOG MARK') + 1 :]
page = until([norm(l) for l in page if not l.startswith('LOG ') and host(l) not in DROP | TIMER])
print(f'desktop {len(desk)}, page {len(page)} requests after MARK')
d = list(difflib.unified_diff(desk, page, 'desktop', 'page', lineterm='', n=1))
print('\n'.join(d[:80]) if d else 'SAME')
sys.exit(1 if d else 0)
