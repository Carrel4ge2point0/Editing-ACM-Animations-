"""Builds editor/squelette.json (bone ids, names, parents, rest pose) from the game's human skeleton.
1. Extract the skeleton with the forge reader (game folder must be readable):
     forge-reader extraire "<Mirage>/DataPC.forge" 0x18cc2999098 skeleton.bin
   (CHR_BaseSkeleton_Human_FULL, type 24aecb7c, 58 bones)
2. python make_skeleton.py skeleton.bin ../editor/squelette.json
"""
import json, os, sys
sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
import squelette as S
bones = S.lire(sys.argv[1], json.load(open(os.path.join(os.path.dirname(os.path.abspath(__file__)), 'noms_os.json'))))
json.dump([{'i': o['i'], 'id': int(o['id'], 16), 'nom': o['nom'], 'parent': o['parent'], 'rot': list(o['rot']), 'pos': list(o['pos'])} for o in bones],
          open(sys.argv[2], 'w'))
print('%d bones -> %s' % (len(bones), sys.argv[2]))
