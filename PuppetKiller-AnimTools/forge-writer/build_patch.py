"""Builds a small DataPC_extra_patch_02.forge that replaces animations by ID.
Reads only; writes a new file (refuses to overwrite).

  python build_patch.py --model "<Mirage>/DataPC_extra_DX12.forge" \
      --like "<an existing working DataPC_extra_patch_02.forge, e.g. Puppet Native's>" \
      --out DataPC_extra_patch_02.forge \
      --anim 0x7fc9c4eed:anims/7fc9c4eed.bin [--anim ...]

--model : the game's small official forge, used for the file header.
--like  : a patch_02 the game already accepts; its 0x10 entry (manifest) and header
          bytes 0x1f-0x21 are copied (our first files without them were ignored).
Each animation is stored uncompressed (raw size = stored size, Adler-32 starting at 0).
"""
import argparse, os, struct, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from ecrire_forge import fabriquer, relire

a = argparse.ArgumentParser()
a.add_argument('--model', required=True); a.add_argument('--like', required=True)
a.add_argument('--out', required=True); a.add_argument('--anim', action='append', required=True)
x = a.parse_args()
if os.path.exists(x.out): sys.exit('refusing: %s already exists' % x.out)
like = open(x.like, 'rb').read()
n = struct.unpack_from('<I', like, 0x41a)[0]
ent = struct.unpack_from('<Q', like, 0x41a + 0x34)[0]
manifest = None
for k in range(n):
    p, i, t = struct.unpack_from('<QQI', like, ent + 20 * k)
    if i == 0x10: manifest = like[p:p + t]
if manifest is None: sys.exit('no 0x10 entry in --like file')
anims = []
for s in x.anim:
    ident, path = s.split(':', 1)
    anims.append((int(ident, 16), open(path, 'rb').read()))
b = bytearray(fabriquer(open(x.model, 'rb').read(), manifest, anims))
b[0x1f:0x22] = like[0x1f:0x22]
r = relire(bytes(b))
for i, d in anims: assert r[i] == d, 'read-back differs for %#x' % i
open(x.out, 'xb').write(b)
print('%s: %d bytes, %d animation(s), read-back OK' % (x.out, len(b), len(anims)))
