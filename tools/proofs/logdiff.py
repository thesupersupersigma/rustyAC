import sys
a = open(sys.argv[1], encoding='utf-8', errors='replace').read().split('\n')
b = open(sys.argv[2], encoding='utf-8', errors='replace').read().split('\n')
n = min(len(a), len(b))
at = next((i for i in range(n) if a[i] != b[i]), None)
if at is None and len(a) == len(b):
    print('IDENTICAL: %d lines' % len(a)); sys.exit(0)
if at is None: at = n
mark = next((a[i] for i in range(min(at, len(a) - 1), -1, -1) if a[i].startswith('#')), '')
print('DIFFERENT at line %d (after "%s"); %d against %d lines' % (at + 1, mark, len(a), len(b)))
for i in range(max(0, at - 2), min(max(len(a), len(b)), at + 4)):
    x = a[i] if i < len(a) else '<end>'; y = b[i] if i < len(b) else '<end>'
    print(('   ' + x[:230]) if x == y else (' a: ' + x[:230] + '\n b: ' + y[:230]))
sys.exit(1)
