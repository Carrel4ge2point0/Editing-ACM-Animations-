//! Lecteur de fichiers .forge d'AC Mirage. Lecture seule : aucun fichier n'est ouvert en écriture.
//! Première étape : parcourir la table et compter les méthodes de compression.

use std::collections::BTreeMap;
use std::env;
use std::fs::File;
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::Path;

const MAGIE_BLOC: [u8; 8] = [0x33, 0xAA, 0xFB, 0x57, 0x99, 0xFA, 0x04, 0x10];

fn u32_a(b: &[u8], i: usize) -> u32 { u32::from_le_bytes(b[i..i + 4].try_into().unwrap()) }
fn u64_a(b: &[u8], i: usize) -> u64 { u64::from_le_bytes(b[i..i + 8].try_into().unwrap()) }

fn lire_a(f: &mut File, pos: u64, buf: &mut [u8]) -> io::Result<()> {
    f.seek(SeekFrom::Start(pos))?;
    f.read_exact(buf)
}

struct Entree { position: u64, identifiant: u64, taille: u32 }

fn table(f: &mut File, taille_fichier: u64) -> Result<Vec<Entree>, String> {
    let mut tete = [0u8; 21];
    lire_a(f, 0, &mut tete).map_err(|e| e.to_string())?;
    if &tete[..9] != b"scimitar\0" { return Err("pas un fichier forge".into()); }
    let version = u32_a(&tete, 9);
    let debut = u64_a(&tete, 13);
    let mut t = [0u8; 0x3C];
    lire_a(f, debut, &mut t).map_err(|e| e.to_string())?;
    let nombre = u32_a(&t, 0) as usize;
    let entrees = u64_a(&t, 0x34);
    println!("  version={version:#x} table={debut:#x} entrees_a={entrees:#x} nombre={nombre}");
    if entrees >= taille_fichier || nombre as u64 * 20 > taille_fichier {
        return Err(format!("table incohérente : {entrees:#x}, {nombre} entrées"));
    }
    let mut brut = vec![0u8; nombre * 20];
    lire_a(f, entrees, &mut brut).map_err(|e| e.to_string())?;
    Ok(brut.chunks_exact(20).map(|c| Entree {
        position: u64_a(c, 0), identifiant: u64_a(c, 8), taille: u32_a(c, 16),
    }).collect())
}

fn analyser(chemin: &Path, stats: &mut Stats) -> Result<(), String> {
    let mut f = File::open(chemin).map_err(|e| e.to_string())?;
    let taille_fichier = f.metadata().map_err(|e| e.to_string())?.len();
    println!("{} ({taille_fichier} octets)", chemin.file_name().unwrap().to_string_lossy());
    let liste = table(&mut f, taille_fichier)?;

    let (mut couvert, mut fin_max, mut trous, mut hors) = (0u64, 0u64, 0usize, 0usize);
    let mut types: BTreeMap<String, (usize, u64)> = BTreeMap::new();
    let mut tete = [0u8; 16];
    let mut precedent: Option<u64> = None;
    for e in &liste {
        let fin = e.position + e.taille as u64;
        if fin > taille_fichier { hors += 1; continue; }
        parcourir(&mut f, e, stats).map_err(|x| x.to_string())?;
        if let Some(p) = precedent { if p != e.position { trous += 1; } }
        precedent = Some(fin);
        couvert += e.taille as u64;
        fin_max = fin_max.max(fin);
        let cle = if e.taille >= 16 && lire_a(&mut f, e.position, &mut tete).is_ok() {
            if tete[..8] == MAGIE_BLOC {
                format!("bloc version={} compression={}", u16::from_le_bytes([tete[8], tete[9]]), tete[10])
            } else {
                "autre (pas de bloc)".to_string()
            }
        } else { "trop petit".to_string() };
        let t = types.entry(cle).or_default();
        t.0 += 1;
        t.1 += e.taille as u64;
    }
    if let Some(e) = liste.first() {
        println!("  premiere: position={:#x} id={:#x} taille={}", e.position, e.identifiant, e.taille);
    }
    println!("  couvert={couvert} ({:.1} %) fin_max={fin_max:#x} ruptures={trous} hors_fichier={hors}",
        couvert as f64 * 100.0 / taille_fichier as f64);
    for (cle, (n, o)) in &types { println!("  {cle} : {n} entrées, {o} octets"); }
    Ok(())
}

/// Parcourt les blocs compressés d'une entrée : magie, version u16, méthode u8, taille max u32,
/// nombre de morceaux u32, puis (taille brute u32, taille compressée u32) par morceau,
/// et enfin, pour chaque morceau, une somme de contrôle u32 suivie des données.
fn parcourir(f: &mut File, e: &Entree, stats: &mut Stats) -> io::Result<()> {
    let fin = e.position + e.taille as u64;
    let mut pos = e.position;
    let mut blocs = 0u32;
    while pos + 19 <= fin {
        let mut t = [0u8; 19];
        lire_a(f, pos, &mut t)?;
        if t[..8] != MAGIE_BLOC { break; }
        let methode = t[10];
        let n = u32_a(&t, 15) as u64;
        if n == 0 || n > 1 << 20 || pos + 19 + 8 * n > fin { stats.tables_fausses += 1; return Ok(()); }
        let mut table = vec![0u8; 8 * n as usize];
        lire_a(f, pos + 19, &mut table)?;
        let mut cur = pos + 19 + 8 * n;
        for c in table.chunks_exact(8) {
            let (brut, comp) = (u32_a(c, 0) as u64, u32_a(c, 4) as u64);
            if cur + 4 + comp > fin { stats.tables_fausses += 1; return Ok(()); }
            let mut d = [0u8; 2];
            let cle = if brut == comp { format!("m{methode} stocke") }
                else if comp >= 2 && lire_a(f, cur + 4, &mut d).is_ok() { format!("m{methode} {:02X} {:02X}", d[0], d[1]) }
                else { format!("m{methode} court") };
            let s = stats.signatures.entry(cle).or_default();
            s.0 += 1; s.1 += comp; s.2 += brut;
            cur += 4 + comp;
        }
        pos = cur;
        blocs += 1;
    }
    *stats.blocs_par_entree.entry(blocs).or_default() += 1;
    if pos == fin { stats.exactes += 1 } else { stats.reste += 1 }
    Ok(())
}

#[derive(Default)]
struct Stats {
    signatures: BTreeMap<String, (u64, u64, u64)>,
    blocs_par_entree: BTreeMap<u32, u64>,
    exactes: u64, reste: u64, tables_fausses: u64,
}

/// Décompresse un bloc à la position donnée. Rend ses données et la position qui suit.
fn lire_bloc(f: &mut File, pos: u64, fin: u64, limite: usize) -> Result<(Vec<u8>, u64, u8), String> {
    let mut t = [0u8; 19];
    lire_a(f, pos, &mut t).map_err(|e| e.to_string())?;
    if t[..8] != MAGIE_BLOC { return Err("pas de bloc ici".into()); }
    let methode = t[10];
    let n = u32_a(&t, 15) as u64;
    if n == 0 || n > 1 << 20 || pos + 19 + 8 * n > fin { return Err("table de bloc incohérente".into()); }
    let mut table = vec![0u8; 8 * n as usize];
    lire_a(f, pos + 19, &mut table).map_err(|e| e.to_string())?;
    let mut cur = pos + 19 + 8 * n;
    let mut sortie = Vec::new();
    for (rang, c) in table.chunks_exact(8).enumerate() {
        let (brut, comp) = (u32_a(c, 0) as usize, u32_a(c, 4) as usize);
        if cur + 4 + comp as u64 > fin { return Err("morceau hors de l'entrée".into()); }
        if rang >= limite { cur += 4 + comp as u64; continue; }
        let mut d = vec![0u8; comp];
        lire_a(f, cur + 4, &mut d).map_err(|e| e.to_string())?;
        if brut == comp {
            sortie.extend_from_slice(&d);
        } else {
            let debut = sortie.len();
            sortie.resize(debut + brut, 0);
            let tampon = &mut sortie[debut..];
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                oozextract::Extractor::new().read_from_slice(&d, tampon)
            }));
            match r {
                Ok(Ok(n)) if n == brut => {}
                Ok(Ok(n)) => return Err(format!("longueur {n} au lieu de {brut}")),
                Ok(Err(e)) => return Err(format!("décompression : {e:?}")),
                Err(_) => return Err("décompression : plantage".into()),
            }
        }
        cur += 4 + comp as u64;
    }
    Ok((sortie, cur, methode))
}

fn lisible(b: &[u8], max: usize) -> String {
    b.iter().take(max).map(|&c| if (32..127).contains(&c) { c as char } else { '.' }).collect()
}

/// Essai : décompresse toutes les entrées d'un fichier et montre les premières.
fn essai(chemin: &Path, montrer: usize) -> Result<(), String> {
    let mut f = File::open(chemin).map_err(|e| e.to_string())?;
    let taille = f.metadata().map_err(|e| e.to_string())?.len();
    let liste = table(&mut f, taille)?;
    let (mut ok, mut ko, mut total, mut vus) = (0u64, 0u64, 0u64, 0usize);
    let mut erreurs: BTreeMap<String, u64> = BTreeMap::new();
    for e in &liste {
        let fin = e.position + e.taille as u64;
        let mut pos = e.position;
        let mut blocs: Vec<Vec<u8>> = Vec::new();
        let mut echec = false;
        while pos + 19 <= fin {
            let mut m = [0u8; 8];
            if lire_a(&mut f, pos, &mut m).is_err() || m != MAGIE_BLOC { break; }
            match lire_bloc(&mut f, pos, fin, usize::MAX) {
                Ok((d, suite, _)) => { pos = suite; blocs.push(d); }
                Err(x) => { *erreurs.entry(x).or_default() += 1; echec = true; break; }
            }
        }
        if echec { ko += 1; continue; }
        if blocs.is_empty() { continue; }
        ok += 1;
        total += blocs.iter().map(|b| b.len() as u64).sum::<u64>();
        if vus < montrer {
            vus += 1;
            println!("id={:#x} blocs={:?}", e.identifiant, blocs.iter().map(|b| b.len()).collect::<Vec<_>>());
            for b in &blocs { println!("    {}", lisible(b, 110)); }
        }
    }
    println!("{} : decompressees={ok} echecs={ko} octets={total}", chemin.file_name().unwrap().to_string_lossy());
    for (x, n) in &erreurs { println!("  erreur {x} : {n}"); }
    Ok(())
}

/// Écrit une ligne par entrée : fichier, identifiant, type, taille décompressée du début lu, nom.
/// Le nom est au début du dernier bloc : type u32, taille u32, longueur u32, puis le texte.
fn indexer(chemin: &Path, sortie: &mut impl Write, bilan: &mut (u64, u64, u64)) -> Result<(), String> {
    let mut f = File::open(chemin).map_err(|e| e.to_string())?;
    let taille = f.metadata().map_err(|e| e.to_string())?.len();
    let liste = table(&mut f, taille)?;
    let fichier = chemin.file_name().unwrap().to_string_lossy().to_string();
    for e in &liste {
        let fin = e.position + e.taille as u64;
        let (mut pos, mut dernier) = (e.position, None);
        while pos + 19 <= fin {
            let mut m = [0u8; 8];
            if lire_a(&mut f, pos, &mut m).is_err() || m != MAGIE_BLOC { break; }
            match lire_bloc(&mut f, pos, fin, 1) {
                Ok((d, suite, _)) => { pos = suite; dernier = Some(d); }
                Err(_) => { dernier = None; bilan.2 += 1; break; }
            }
        }
        let Some(d) = dernier else { continue };
        if d.len() < 12 { continue; }
        let n = u32_a(&d, 8) as usize;
        let nom = if n > 0 && n < 512 && 12 + n <= d.len() && d[12..12 + n].iter().all(|c| (32..127).contains(c)) {
            String::from_utf8_lossy(&d[12..12 + n]).to_string()
        } else { String::new() };
        if nom.is_empty() { bilan.1 += 1 } else { bilan.0 += 1 }
        writeln!(sortie, "{fichier}\t{:#x}\t{:08x}\t{}\t{nom}", e.identifiant, u32_a(&d, 0), e.taille).map_err(|x| x.to_string())?;
    }
    Ok(())
}

/// Écrit dans un fichier les données décompressées du dernier bloc de l'entrée `id`.
fn extraire(chemin: &Path, id: u64, sortie: &Path) -> Result<usize, String> {
    let mut f = File::open(chemin).map_err(|e| e.to_string())?;
    let taille = f.metadata().map_err(|e| e.to_string())?.len();
    let liste = table(&mut f, taille)?;
    let e = liste.iter().find(|e| e.identifiant == id).ok_or("identifiant introuvable")?;
    let fin = e.position + e.taille as u64;
    let (mut pos, mut dernier) = (e.position, None);
    while pos + 19 <= fin {
        let mut m = [0u8; 8];
        if lire_a(&mut f, pos, &mut m).is_err() || m != MAGIE_BLOC { break; }
        let (d, suite, _) = lire_bloc(&mut f, pos, fin, usize::MAX)?;
        pos = suite;
        dernier = Some(d);
    }
    let d = dernier.ok_or("aucun bloc")?;
    std::fs::write(sortie, &d).map_err(|e| e.to_string())?;
    Ok(d.len())
}

/// Extrait une liste d'entrées (lignes « fichier.forge<TAB>id<TAB>nom ») vers un dossier, une table lue par fichier.
fn extraire_lot(jeu: &Path, liste: &Path, dossier: &Path) -> Result<(usize, usize), String> {
    let texte = std::fs::read_to_string(liste).map_err(|e| e.to_string())?;
    let mut par_fichier: BTreeMap<String, Vec<(u64, String)>> = BTreeMap::new();
    for l in texte.lines() {
        let c: Vec<&str> = l.trim_start_matches('\u{feff}').split('\t').collect();
        if c.len() < 3 { continue; }
        let id = u64::from_str_radix(c[1].trim_start_matches("0x"), 16).map_err(|e| e.to_string())?;
        par_fichier.entry(c[0].to_string()).or_default().push((id, c[2].to_string()));
    }
    std::fs::create_dir_all(dossier).map_err(|e| e.to_string())?;
    let (mut ok, mut ko) = (0, 0);
    for (fichier, voulus) in &par_fichier {
        let chemin = jeu.join(fichier);
        let mut f = File::open(&chemin).map_err(|e| e.to_string())?;
        let taille = f.metadata().map_err(|e| e.to_string())?.len();
        let liste = table(&mut f, taille)?;
        let index: BTreeMap<u64, &Entree> = liste.iter().map(|e| (e.identifiant, e)).collect();
        for (id, nom) in voulus {
            let Some(e) = index.get(id) else { ko += 1; continue };
            let fin = e.position + e.taille as u64;
            let (mut pos, mut dernier) = (e.position, None);
            while pos + 19 <= fin {
                let mut m = [0u8; 8];
                if lire_a(&mut f, pos, &mut m).is_err() || m != MAGIE_BLOC { break; }
                match lire_bloc(&mut f, pos, fin, usize::MAX) {
                    Ok((d, suite, _)) => { pos = suite; dernier = Some(d); }
                    Err(_) => { dernier = None; break; }
                }
            }
            let propre: String = nom.chars().map(|c| if c.is_ascii_alphanumeric() || c == '_' || c == '-' { c } else { '_' }).collect();
            match dernier {
                Some(d) => { std::fs::write(dossier.join(format!("{propre}.bin")), &d).map_err(|e| e.to_string())?; ok += 1; }
                None => ko += 1,
            }
        }
    }
    Ok((ok, ko))
}


/// Cherche, dans toutes les ressources décompressées des forges données, celles qui contiennent
/// un des identifiants (u64) listés. Lecture seule. Sortie TSV : forge, ressource, identifiants trouvés.
fn chercher(forges: &[String], ids: &std::collections::HashSet<u64>, sortie: &mut impl Write) -> Result<(u64, u64), String> {
    let (mut vues, mut trouvees) = (0u64, 0u64);
    for chemin in forges {
        let mut f = File::open(chemin).map_err(|e| format!("{chemin}: {e}"))?;
        let taille = f.metadata().map_err(|e| e.to_string())?.len();
        let liste = match table(&mut f, taille) { Ok(l) => l, Err(e) => { println!("{chemin} : {e}"); continue } };
        let nom = Path::new(chemin).file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        for e in &liste {
            let fin = e.position + e.taille as u64;
            let mut pos = e.position;
            let mut tout: Vec<u8> = Vec::new();
            while pos + 19 <= fin {
                let mut m = [0u8; 8];
                if lire_a(&mut f, pos, &mut m).is_err() || m != MAGIE_BLOC { break; }
                match lire_bloc(&mut f, pos, fin, usize::MAX) { Ok((d, suite, _)) => { pos = suite; tout.extend_from_slice(&d); } Err(_) => break }
            }
            vues += 1;
            if tout.len() < 8 { continue; }
            let mut vus: Vec<u64> = Vec::new();
            for i in 0..tout.len() - 7 {
                let v = u64_a(&tout, i);
                if v != e.identifiant && ids.contains(&v) && !vus.contains(&v) { vus.push(v); }
            }
            if !vus.is_empty() {
                trouvees += 1;
                let l: Vec<String> = vus.iter().map(|v| format!("{v:#x}")).collect();
                writeln!(sortie, "{nom}\t{:#x}\t{}\t{}", e.identifiant, tout.len(), l.join(",")).map_err(|e| e.to_string())?;
            }
        }
        sortie.flush().ok();
        println!("{nom} : {} ressources", liste.len());
    }
    Ok((vues, trouvees))
}

fn main() {
    let args: Vec<String> = env::args().skip(1).collect();
    if args.is_empty() { eprintln!("usage : forge-lecteur FICHIER.forge..."); std::process::exit(2); }
    if args[0] == "index" {
        std::panic::set_hook(Box::new(|_| {}));
        let mut sortie = BufWriter::new(File::create(&args[1]).expect("création de l'index"));
        let mut bilan = (0u64, 0u64, 0u64);
        for a in &args[2..] { if let Err(e) = indexer(Path::new(a), &mut sortie, &mut bilan) { println!("{a} : ERREUR {e}"); } }
        sortie.flush().expect("écriture de l'index");
        println!("BILAN noms={} sans_nom={} echecs={}", bilan.0, bilan.1, bilan.2);
        return;
    }
    if args[0] == "extraire-lot" {
        std::panic::set_hook(Box::new(|_| {}));
        match extraire_lot(Path::new(&args[1]), Path::new(&args[2]), Path::new(&args[3])) {
            Ok((ok, ko)) => println!("extraites={ok} echecs={ko}"),
            Err(e) => { println!("ERREUR {e}"); std::process::exit(1); }
        }
        return;
    }
    if args[0] == "chercher" {
        // chercher SORTIE.tsv IDS.txt FORGE...   (IDS.txt : un identifiant hexadécimal par ligne)
        let ids: std::collections::HashSet<u64> = std::fs::read_to_string(&args[2]).expect("liste d'identifiants").lines()
            .filter_map(|l| u64::from_str_radix(l.trim().trim_start_matches("0x"), 16).ok()).collect();
        let mut sortie = BufWriter::new(File::create(&args[1]).expect("sortie"));
        match chercher(&args[3..], &ids, &mut sortie) { Ok((v, t)) => println!("vues={v} trouvees={t}"), Err(e) => println!("ERREUR {e}") }
        return;
    }
    if args[0] == "extraire" {
        let id = u64::from_str_radix(args[2].trim_start_matches("0x"), 16).expect("identifiant hexadécimal");
        match extraire(Path::new(&args[1]), id, Path::new(&args[3])) {
            Ok(n) => println!("{n} octets"),
            Err(e) => { println!("ERREUR {e}"); std::process::exit(1); }
        }
        return;
    }
    if args[0] == "essai" {
        std::panic::set_hook(Box::new(|_| {}));
        for a in &args[1..] { if let Err(e) = essai(Path::new(a), 14) { println!("{a} : ERREUR {e}"); } }
        return;
    }
    let mut erreurs = 0;
    let mut stats = Stats::default();
    for a in &args {
        if let Err(e) = analyser(Path::new(a), &mut stats) { println!("{a} : ERREUR {e}"); erreurs += 1; }
    }
    println!("TOTAL entrees_exactes={} reste={} tables_fausses={}", stats.exactes, stats.reste, stats.tables_fausses);
    println!("blocs par entree : {:?}", stats.blocs_par_entree);
    let mut v: Vec<_> = stats.signatures.iter().collect();
    v.sort_by_key(|(_, s)| std::cmp::Reverse(s.0));
    for (k, (n, c, b)) in v.iter().take(30) { println!("{k} : {n} morceaux, compresse {c}, brut {b}"); }
    if erreurs > 0 { std::process::exit(1); }
}
