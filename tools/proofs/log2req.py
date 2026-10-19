# SPDX-License-Identifier: GPL-3.0-or-later
# A scripted rustyac-launcher run's --log -> a request list for launcher_oracle and --transcript:
# every ac:// request the theme sent, in order, `start` and the online handlers left out.
#   python log2req.py <run.log> <out.req>
import re
import sys

LINE = re.compile(r'^bridge (\S+)(?: POST (.*))? -> \S+ (?:true|false) \d+$')
# getguid and getachievements ask Steam: the oracle refuses them (Steam is not loaded there)
DROP = {'start', 'getguid', 'getachievements', 'getserver', 'getserverlist', 'getservercached', 'getlanservers', 'pingserver'}

out = ['SETUP x']
record = None
for raw in open(sys.argv[1], encoding='utf-8', errors='replace').read().split('\n'):
    raw = raw.rstrip('\r')
    if raw.startswith('bridge '):
        record = raw
    elif record is not None:
        record += '\n' + raw  # a body with a newline: joined until the tail parses
    else:
        continue
    if record.endswith(' waits'):
        record = None
        continue
    m = LINE.match(record) if '\n' not in record else None
    if not m:
        if '\n' in record and LINE.match(record.replace('\n', ' ')):
            sys.exit(f'a body with a newline, not expressible in a request list: {record[:80]}')
        if record.count('\n') > 50:
            sys.exit(f'unparsed bridge line: {record[:80]}')
        continue
    url, body = m.group(1), m.group(2)
    record = None
    if re.match(r'ac://([^/?]*)', url).group(1) in DROP:
        continue
    out.append(f'GET {url}' if body is None else f'POST {url}\t{body}')
open(sys.argv[2], 'w', encoding='utf-8', newline='\n').write('\n'.join(out) + '\n')
print(f'{len(out) - 1} requests')
