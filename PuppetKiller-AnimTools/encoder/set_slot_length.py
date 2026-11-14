"""The encoder requires the JSON Length to equal the slot (mold) length.
- Animation shorter than the slot: Length is set to the slot length (the game blends out by itself, no freeze).
- Animation longer than the slot: keys past the slot end are removed (end trimmed).
Usage: python set_slot_length.py in.json out.json <slot length in seconds>"""
import json, sys
d = json.load(open(sys.argv[1])); L = float(sys.argv[3]); N = round(L * 60)
cut = 0
for t in d['Tracks']:
    k = t.get('Keyframes') or []; keep = [x for x in k if x['Frame'] <= N]; cut += len(k) - len(keep); t['Keyframes'] = keep
old = d['Length']; d['Length'] = L
json.dump(d, open(sys.argv[2], 'w'))
print('%s: %.3f s -> %.3f s (%d keys past the end removed)' % (sys.argv[2], old, L, cut))
