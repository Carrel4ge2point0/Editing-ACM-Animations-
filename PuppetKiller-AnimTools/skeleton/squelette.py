"""Lit un squelette (type 24aecb7c) : os, parents, pose de repos locale. Lecture seule."""
import re, struct, json, sys, zlib

MARQUE = bytes.fromhex('49107495')

def lire(chemin, noms=None):
    sk = open(chemin, 'rb').read()
    pos = [m.start() for m in re.finditer(re.escape(MARQUE), sk)]
    os_ = []
    for i, m in enumerate(pos):
        h = struct.unpack_from('<I', sk, m + 4)[0]
        p = m + 8
        if sk[p] == 0x03:              # pas de parent
            ref_parent, p = None, p + 1
        else:
            ref_parent, p = struct.unpack_from('<H', sk, p + 1)[0], p + 9
        p += 9                         # seconde référence
        t = struct.unpack_from('<3f', sk, p)          # pose globale (repère du personnage)
        q = struct.unpack_from('<4f', sk, p + 12)
        tl = struct.unpack_from('<3f', sk, p + 28)    # pose locale (par rapport au parent)
        ql = struct.unpack_from('<4f', sk, p + 40)
        obj = struct.unpack_from('<H', sk, m - 8)[0]
        os_.append({'i': i, 'obj': obj, 'id': '%08x' % h, 'nom': (noms or {}).get('%08x' % h, '?'),
                    'ref_parent': ref_parent, 'pos_globale': t, 'rot_globale': q,
                    'pos': tl, 'rot': ql})
    par_obj = {o['obj']: o['i'] for o in os_}
    for o in os_:
        r = o['ref_parent']
        o['parent'] = None if r is None else par_obj.get(r)
    return os_

def qmul(a, b):
    ax, ay, az, aw = a; bx, by, bz, bw = b
    return (aw*bx + ax*bw + ay*bz - az*by, aw*by - ax*bz + ay*bw + az*bx,
            aw*bz + ax*by - ay*bx + az*bw, aw*bw - ax*bx - ay*by - az*bz)

def qrot(q, v):
    x, y, z, w = qmul(qmul(q, (*v, 0.0)), (-q[0], -q[1], -q[2], q[3]))
    return (x, y, z)

def global_(os_):
    g = {}
    for o in os_:
        if o['parent'] is None:
            g[o['i']] = (o['pos'], o['rot'])
        else:
            pp, pq = g[o['parent']]
            d = qrot(pq, o['pos'])
            g[o['i']] = ((pp[0]+d[0], pp[1]+d[1], pp[2]+d[2]), qmul(pq, o['rot']))
    return g

if __name__ == '__main__':
    noms = json.load(open(sys.argv[2])) if len(sys.argv) > 2 else {}
    os_ = lire(sys.argv[1], noms)
    g = global_(os_)
    for o in os_:
        par = os_[o['parent']]['nom'] if o['parent'] is not None and o['parent'] < len(os_) else '-'
        n = sum(x*x for x in o['rot']) ** 0.5
        print('%2d %-18s parent=%-16s ref=%s |q|=%.3f global=(%.3f, %.3f, %.3f)' % (
            o['i'], o['nom'], par, o['ref_parent'], n, *g[o['i']][0]))
