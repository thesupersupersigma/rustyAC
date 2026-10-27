p = 'docs/port/leftovers_42c.md'
s = open(p, 'rb').read().decode('utf-8')
bs = chr(92)
s = s.replace(chr(8) + 'lack', bs + 'black')
open(p, 'wb').write(s.encode('utf-8'))
l = open(p, 'rb').read().split(b'\n')[190]
print(l[60:92].hex(' '))
