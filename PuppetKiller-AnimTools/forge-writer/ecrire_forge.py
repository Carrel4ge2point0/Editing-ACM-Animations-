"""Fabrique un petit fichier .forge d'AC Mirage à partir de ressources d'animation .bin.

Conversion de fichiers uniquement : l'outil lit des fichiers et en écrit un nouveau,
dans le dossier indiqué. Il n'installe rien et ne touche pas au dossier du jeu.

Structure reprise des petits .forge officiels (ex. DataPC_extra_DX12.forge) :
- en-tête « scimitar » de 0x41a octets, repris d'un fichier modèle ;
- descripteur à 0x41a (0x2c octets), second descripteur à 0x446 (0x30 octets) ;
- table d'entrées à 0x476 : position u64, identifiant u64, taille u32 ;
- place réservée (capacité × 20 octets), un octet, puis les données ;
- fichier complété par des zéros jusqu'à un multiple de 0x8000.
Une entrée d'animation = deux blocs : fiche de 16 octets (01 00, identifiant u64,
taille u32, 00 00) puis la ressource. Chaque bloc a un seul morceau stocké tel quel
(taille brute = taille stockée), précédé d'une somme Adler-32 partant de 0.

Usage :
  ecrire_forge.py MODELE.forge SORTIE.forge --manifeste FICHIER_0x10.bin \
      --anim 0xIDENTIFIANT:ressource.bin [--anim ...]
"""
import argparse, os, struct, sys, zlib

MAGIE = bytes.fromhex('33aafb5799fa0410')
ENTETE_BLOC = bytes.fromhex('030008000004 80'.replace(' ', ''))  # version 3, méthode 8, taille max
TABLE, DESC2, ENTREES = 0x41a, 0x446, 0x476


def bloc_stocke(donnees: bytes) -> bytes:
    if len(donnees) > 0x40000:
        raise ValueError('ressource trop grande pour un seul morceau (%d octets)' % len(donnees))
    return (MAGIE + ENTETE_BLOC + struct.pack('<III', 1, len(donnees), len(donnees))
            + struct.pack('<I', zlib.adler32(donnees, 0)) + donnees)


def entree_animation(ident: int, ressource: bytes, modele_fiche: bytes = None) -> bytes:
    """Fiche officielle de 16 octets, ou copie d'une fiche modèle (ex. fiche de 114 octets
    écrite par AnvilToolkit) dont seuls l'identifiant et la taille sont remplacés."""
    tete = struct.pack('<HQI', 1, ident, len(ressource))
    if modele_fiche is None:
        fiche = tete + b'\0\0'
    else:
        if modele_fiche[:2] != b'\1\0' or len(modele_fiche) < 16:
            raise ValueError('fiche modèle inattendue')
        fiche = tete + modele_fiche[14:]
    return bloc_stocke(fiche) + bloc_stocke(ressource)


def fiche_de(base: bytes, ident: int) -> bytes:
    """Données du premier bloc (stocké) de l'entrée `ident` d'un .forge."""
    u32 = lambda i: struct.unpack_from('<I', base, i)[0]
    for k in range(u32(TABLE)):
        p, i, t = struct.unpack_from('<QQI', base, ENTREES + 20 * k)
        if i == ident:
            br, co = struct.unpack_from('<II', base, p + 19)
            if base[p:p + 8] != MAGIE or u32(p + 15) != 1 or br != co:
                raise ValueError('fiche non stockée')
            return base[p + 31:p + 31 + co]
    raise ValueError('identifiant absent')


def fabriquer(modele: bytes, manifeste: bytes, anims: list) -> bytes:
    if modele[:9] != b'scimitar\0' or struct.unpack_from('<Q', modele, 13)[0] != TABLE:
        raise ValueError('modèle inattendu')
    # Ordre des données comme dans DataPC_extra_DX12 : ressources, 0x91, puis 0x10.
    contenus = [(i, entree_animation(i, r)) for i, r in anims] + [(0x91, b'\0' * 4), (0x10, manifeste)]
    n = len(contenus)
    capacite = n + 6  # même marge que les fichiers officiels (3 entrées -> 9)
    debut = ENTREES + 20 * capacite + 1
    pos, table, corps = debut, b'', b''
    for ident, c in contenus:
        table += struct.pack('<QQI', pos, ident, len(c))
        corps += c
        pos += len(c)
    sortie = bytearray(modele[:TABLE])
    sortie += (struct.pack('<I', n) + b'\0' * 12 + b'\xff' * 12 + struct.pack('<II', capacite, 1)
               + struct.pack('<Q', DESC2))
    sortie += (struct.pack('<Q', n) + struct.pack('<Q', ENTREES) + b'\xff' * 8
               + struct.pack('<II', 0, capacite - 1) + b'\xff' * 16)
    assert len(sortie) == ENTREES
    sortie += table + b'\0' * (debut - ENTREES - len(table)) + corps
    sortie += b'\0' * (-len(sortie) % 0x8000)
    return bytes(sortie)


def ajouter_a_un_forge(base: bytes, anims: list, modele_fiche: bytes = None) -> bytes:
    """Recopie toutes les entrées d'un .forge existant, octet pour octet, et ajoute des
    animations avant ses entrées de service. Disposition comme le fichier de base :
    données serrées juste après la table, même capacité si elle suffit."""
    u32 = lambda i: struct.unpack_from('<I', base, i)[0]
    n0, ent0 = u32(TABLE), struct.unpack_from('<Q', base, TABLE + 0x34)[0]
    if ent0 != ENTREES:
        raise ValueError('base inattendue')
    anciennes = [struct.unpack_from('<QQI', base, ENTREES + 20 * k) for k in range(n0)]
    deja = {i for _, i, _ in anciennes}
    if any(i in deja for i, _ in anims):
        raise ValueError('identifiant déjà présent dans la base')
    service = {0x10, 0x91, 0xc1, 0xffa}
    donnees = [(i, base[p:p + t]) for p, i, t in anciennes if i not in service]
    services = [(i, base[p:p + t]) for p, i, t in anciennes if i in service]
    contenus = donnees + [(i, entree_animation(i, r, modele_fiche)) for i, r in anims] + services
    n = len(contenus)
    capacite = max(u32(TABLE + 0x1c), n)
    dernier = struct.unpack_from('<I', base, DESC2 + 0x1c)[0] - n0 + n
    debut = ENTREES + 20 * n + 1
    pos, table, corps = debut, b'', b''
    for ident, c in contenus:
        table += struct.pack('<QQI', pos, ident, len(c))
        corps += c
        pos += len(c)
    sortie = bytearray(base[:TABLE])
    sortie += (struct.pack('<I', n) + b'\0' * 12 + b'\xff' * 12 + struct.pack('<II', capacite, 1)
               + struct.pack('<Q', DESC2))
    sortie += (struct.pack('<Q', n) + struct.pack('<Q', ENTREES) + b'\xff' * 8
               + struct.pack('<II', 0, dernier) + b'\xff' * 16)
    sortie += table + b'\0' + corps
    sortie += b'\0' * (-len(sortie) % 0x10000)
    return bytes(sortie)


def decompresser_entrees(base: bytes, ressources: dict) -> bytes:
    """Diagnostic : recopie un .forge en remplaçant le dernier bloc de chaque entrée listée
    dans `ressources` (identifiant -> données décompressées) par un bloc stocké tel quel.
    Le premier bloc (fiche) est conservé octet pour octet."""
    u32 = lambda i: struct.unpack_from('<I', base, i)[0]
    n0 = u32(TABLE)
    anciennes = [struct.unpack_from('<QQI', base, ENTREES + 20 * k) for k in range(n0)]
    contenus = []
    for p, i, t in anciennes:
        c = base[p:p + t]
        if i in ressources:
            if c[:8] != MAGIE:
                raise ValueError('entrée %#x sans bloc' % i)
            m = struct.unpack_from('<I', c, 15)[0]
            fin0 = 19 + 8 * m + sum(4 + struct.unpack_from('<II', c, 19 + 8 * k)[1] for k in range(m))
            c = c[:fin0] + bloc_stocke(ressources[i])
        contenus.append((i, c))
    debut = ENTREES + 20 * n0 + 1
    pos, table, corps = debut, b'', b''
    for ident, c in contenus:
        table += struct.pack('<QQI', pos, ident, len(c))
        corps += c
        pos += len(c)
    sortie = bytearray(base[:ENTREES]) + table + b'\0' + corps
    sortie += b'\0' * (-len(sortie) % 0x10000)
    return bytes(sortie)


def relire(b: bytes) -> dict:
    """Relecture indépendante : rend {identifiant: données du dernier bloc}."""
    u32 = lambda i: struct.unpack_from('<I', b, i)[0]
    n, ent = u32(TABLE), struct.unpack_from('<Q', b, TABLE + 0x34)[0]
    res = {}
    for k in range(n):
        p, i, t = struct.unpack_from('<QQI', b, ent + 20 * k)
        fin, dernier = p + t, None
        while p + 19 <= fin and b[p:p + 8] == MAGIE:
            m = u32(p + 15)
            cur = p + 19 + 8 * m
            d = b''
            for c in range(m):
                br, co = struct.unpack_from('<II', b, p + 19 + 8 * c)
                if br != co or zlib.adler32(b[cur + 4:cur + 4 + co], 0) != u32(cur):
                    raise ValueError('morceau invalide dans %#x' % i)
                d += b[cur + 4:cur + 4 + co]
                cur += 4 + co
            p, dernier = cur, d
        res[i] = dernier if dernier is not None else b[p:fin]
    return res


if __name__ == '__main__':
    a = argparse.ArgumentParser()
    a.add_argument('modele'); a.add_argument('sortie')
    a.add_argument('--manifeste', required=True)
    a.add_argument('--anim', action='append', required=True)
    x = a.parse_args()
    if os.path.exists(x.sortie):
        sys.exit('refus : %s existe déjà' % x.sortie)
    anims = []
    for s in x.anim:
        ident, chemin = s.split(':', 1)
        anims.append((int(ident, 16), open(chemin, 'rb').read()))
    b = fabriquer(open(x.modele, 'rb').read(), open(x.manifeste, 'rb').read(), anims)
    relu = relire(b)
    for i, r in anims:
        assert relu[i] == r, 'relecture différente pour %#x' % i
    with open(x.sortie, 'xb') as f:
        f.write(b)
    print('%s : %d octets, %d animation(s), relecture identique' % (x.sortie, len(b), len(anims)))
