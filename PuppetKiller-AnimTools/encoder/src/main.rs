//! Conversion hors jeu, à partir d'un moule. Aucun accès au processus du jeu.
mod pn;
use serde_json::{json, Value};
use std::{env, fs, path::Path};
type Res<T> = Result<T, String>;
fn u32at(b: &[u8], p: usize) -> Res<u32> {
    Ok(u32::from_le_bytes(b.get(p..p+4).ok_or("u32 tronqué")?.try_into().unwrap()))
}
fn put(b: &mut [u8], p: usize, n: usize) -> Res<()> {
    let n = u32::try_from(n).map_err(|_| "taille hors u32")?;
    b.get_mut(p..p+4).ok_or("champ absent")?.copy_from_slice(&n.to_le_bytes()); Ok(())
}
fn hex(b: &[u8]) -> String { b.iter().map(|x| format!("{x:02x}")).collect() }
fn family(t: u8) -> Option<(u8, usize, usize)> {
    for (base,k) in [(3,2),(6,3),(9,4),(12,6),(15,8),(18,1),(21,2),(24,12),(27,12),(51,1),(60,1)] {
        if t>=base && t<=base+2 {return Some((base,k,(t-base) as usize));}
    } None
}
#[derive(Clone,Debug)]
struct Track { start:usize, end:usize, tag:u8, size:usize, frames:Vec<u32>, data:usize, k:usize, base:u8, editable:bool }
fn track(b:&[u8], p:usize) -> Res<Track> {
    let t=*b.get(p).ok_or("piste absente")?;
    let s=u32at(b,p+1)? as usize;let n=u32at(b,p+5)? as usize;
    if !(1..=100000).contains(&n) {return Err("nombre de clés invalide".into());}
    if t==78 && n<=7 && s==8+8*n {
        let end=p+9+(n-1)+8*n;if end>b.len(){return Err("bloc de fin tronqué".into());}
        return Ok(Track{start:p,end,tag:t,size:s,frames:vec![0;n],data:p+9,k:8,base:t,editable:false});
    }
    let f=family(t);
    if n==1 && (f.is_none() || t>=51) {
        if !(3..64).contains(&s) || t>=0x60 {return Err("piste opaque non reconnue".into());}
        let end=p+9+s-if s%4==0 {4}else{2};
        if end>b.len(){return Err("piste opaque tronquée".into());}
        return Ok(Track{start:p,end,tag:t,size:s,frames:vec![0],data:p+9,k:end-p-9,base:t,editable:false});
    }
    let (base,k,j)=f.ok_or("codec inconnu")?;
    let mut frames=vec![0];let mut d=p+9;
    if n>1 { match j {
        0=>{ for _ in 1..n {frames.push(*b.get(d).ok_or("temps tronqué")? as u32);d+=1;} },
        1=>{for _ in 1..n {let a=b.get(d..d+2).ok_or("temps tronqué")?;frames.push(u16::from_le_bytes(a.try_into().unwrap()) as u32);d+=2;}},
        _=>{let len=(*b.get(d).ok_or("masque absent")? & 63) as usize;d+=1; if len==0{return Err("masque vide".into());}let mask=b.get(d..d+len).ok_or("masque tronqué")?;frames.clear();for (i,v) in mask.iter().enumerate(){for bit in 0..8{if v&(1<<bit)!=0{frames.push(((i*8+bit)*2) as u32);}}}d+=len;if frames.len()!=n{return Err("compte du masque incohérent".into());}}
    }}
    if matches!(base,6|9|12|15|27) && frames.windows(2).any(|w|w[0]>=w[1]) {return Err("temps non croissants".into());}
    let end=d+k*n;if end>b.len(){return Err("clés tronquées".into());}
    let u=match k{4|8|12=>4,2|6=>2,_=>1};
    let expected=k*n+if n==1{u.max(2)}else{let extra=match j{0=>2,1=>3,_=>1};(d-p-9+extra+u-1)/u*u};
    if s!=expected{return Err("taille déclarée de piste incohérente".into());}
    Ok(Track{start:p,end,tag:t,size:s,frames,data:d,k,base,editable:matches!(base,6|9|12|15|27)})
}
struct Model { bytes:Vec<u8>, name:String, duration:f32, start:usize, sub:usize, ids:Vec<u32>, tracks:Vec<Track>, sections:Vec<(u16,usize,usize)> }
fn parse(b:Vec<u8>) -> Res<Model> {
    if u32at(&b,0)?!=0x0fa3067f{return Err("type d'animation incorrect".into());}
    let n=u32at(&b,8)? as usize;if n==0||n>1024{return Err("nom invalide".into());}
    let name=String::from_utf8(b.get(12..12+n).ok_or("nom tronqué")?.to_vec()).map_err(|_|"nom non UTF-8")?;
    let q=12+n;
    if b.get(q..q+4)!=Some(&[1,2,0,0][..]) {return Err("en-tête non reconnu".into());}
    let mut sections=vec![];
    for i in 0..4 {let p=q+14+12*i;let t=u16::from_le_bytes(b.get(p..p+2).ok_or("table tronquée")?.try_into().unwrap());let s=u32at(&b,p+2)? as usize;
        if ![21,7,5,3].contains(&t) || sections.iter().any(|(seen,_,_)|*seen==t) || b.get(p+6..p+12)!=Some(&[0;6][..]){return Err("table non reconnue".into());} sections.push((t,s,p+2));}
    let count=sections.iter().find(|(t,_,_)|*t==21).ok_or("table d’os absente")?.1;if !(3..=400).contains(&count){return Err("nombre d'os non reconnu".into());}
    let mut sub_candidates=vec![];
    for p in q+62..b.len().saturating_sub(8) {if b.get(p..p+4)==Some(&[0x7f,6,0xa3,15][..]) && p>=9 && b[p-9]==1 && u32at(&b,4)? as usize==b.len()-(p-9){sub_candidates.push(p);}}
    if sub_candidates.len()!=1{return Err("sous-objet ambigu ou taille incorrecte".into());}let sub=sub_candidates[0]-9;
    let duration=f32::from_bits(u32at(&b,sub+13)?);if !duration.is_finite()||duration<=0.0{return Err("durée invalide".into());}
    let mut candidates=vec![];
    for p in sub+17..b.len().saturating_sub(4+count*4).min(8192) {
        if u32at(&b,p)? as usize!=count{continue;}
        let ids:Vec<u32>=(0..count).map(|i|u32at(&b,p+4+4*i).unwrap()).collect();
        if ids.windows(2).any(|w|w[0]>w[1]) || ids.windows(2).filter(|w|w[0]!=w[1]).count()<count/2 {continue;}
        let end=p+4+4*count;
        for st in end+12..b.len().saturating_sub(9).min(end+6000) {
            if u32at(&b,st-12)?!=63 || u32at(&b,st-8)? as usize!=b.len()-st+4 {continue;}
            let nt=u32at(&b,st-4)? as usize; if nt>count || count-nt>3 || nt==0 {continue;}
            let mut tracks=vec![];let mut pos=st;
            while pos<b.len() && tracks.len()<nt {match track(&b,pos){Ok(t)=>{pos=t.end;tracks.push(t)},Err(_)=>break}}
            if pos==b.len()&&tracks.len()==nt{candidates.push((st,ids.clone(),tracks));}
        }
    }
    if candidates.len()!=1{return Err(format!("structure non prise en charge : {} candidats complets",candidates.len()));}
    let (start,ids,tracks)=candidates.remove(0);
    Ok(Model{bytes:b,name,duration,start,sub,ids,tracks,sections})
}
fn quat(base:u8, r:&[u8])->Vec<f64>{
    let root=2f64.sqrt();let (mut a,idx)=match base {
        9|15=>{let (u,bits)=if base==9{(u32::from_le_bytes(r.try_into().unwrap()) as u64,10)}else{(((u32::from_le_bytes(r[0..4].try_into().unwrap()) as u64)<<32)|u32::from_le_bytes(r[4..8].try_into().unwrap()) as u64,20)};
            let mask=(1u64<<bits)-1;((0..3).rev().map(|j|((u>>(j*bits))&mask) as f64/mask as f64*root-root/2.).collect::<Vec<_>>(),(u>>(if base==9{30}else{62})) as usize)},
        12=>{let v:Vec<u16>=r.chunks_exact(2).map(|s|u16::from_le_bytes(s.try_into().unwrap())).collect();(v.iter().map(|u|(u&32767)as f64/32767.*root-root/2.).collect(),((v[0]>>15)|((v[1]>>15)<<1))as usize)},
        _=>(r.iter().map(|u|(u&127)as f64/127.*root-root/2.).collect(),((r[0]>>7)|((r[1]>>7)<<1))as usize)
    };let missing=(1.-a.iter().map(|v|v*v).sum::<f64>()).max(0.).sqrt();a.insert(idx,missing);a
}
fn values(m:&Model,t:&Track,i:usize)->Vec<f64>{let r=&m.bytes[t.data+i*t.k..t.data+(i+1)*t.k];if t.base==27{r.chunks_exact(4).map(|v|f32::from_le_bytes(v.try_into().unwrap())as f64).collect()}else{quat(t.base,r)}}
fn document(m:&Model)->Value{
    json!({"Format":"PuppetKiller-animation-1","Name":m.name,"Duration":m.duration,"Tracks":m.tracks.iter().enumerate().map(|(idx,t)|{
        if t.editable{let key=if t.base==27{"LocalPosition"}else{"LocalRotation"};json!({"TrackIndex":idx,"TrackID":m.ids[idx].to_string(),"codec":t.tag,"Keyframes":t.frames.iter().enumerate().map(|(i,f)|{let mut v=json!({"Frame":f});v[key]=json!(values(m,t,i));v}).collect::<Vec<_>>()})}
        else{json!({"TrackIndex":idx,"TrackID":m.ids[idx].to_string(),"codec":t.tag,"PreserveHex":hex(&m.bytes[t.start..t.end])})}
    }).collect::<Vec<_>>()})
}
fn number(v:&Value)->Res<u32>{u32::try_from(v.as_u64().ok_or("entier positif attendu")?).map_err(|_|"entier trop grand".into())}
fn vector(v:&Value,n:usize)->Res<Vec<f64>>{let a=v.as_array().ok_or("vecteur absent")?;if a.len()!=n{return Err("dimension incorrecte".into());}a.iter().map(|v|{let f=v.as_f64().ok_or("nombre attendu")?;if !f.is_finite(){Err("nombre non fini".into())}else{Ok(f)}}).collect()}
fn pack_quat(base:u8, mut q:Vec<f64>)->Res<Vec<u8>>{
    let norm=q.iter().map(|v|v*v).sum::<f64>().sqrt();if (norm-1.).abs()>0.02{return Err("rotation non unitaire (tolérance 0,02)".into());}for x in &mut q{*x/=norm;}
    let idx=(0..4).max_by(|a,b|q[*a].abs().total_cmp(&q[*b].abs())).unwrap();if q[idx]<0.{for x in &mut q{*x = -*x;}}
    q.remove(idx);let bits=match base{6=>7,9=>10,12=>15,15=>20,_=>return Err("rotation inconnue".into())};let max=(1u64<<bits)-1;
    let f:Vec<u64>=q.iter().map(|v|((v/2f64.sqrt()+0.5)*max as f64).round().clamp(0.,max as f64)as u64).collect();
    Ok(match base{
        6=>vec![(f[0]|(((idx&1)as u64)<<7))as u8,(f[1]|(((idx>>1)as u64)<<7))as u8,f[2]as u8],
        12=>[f[0]|(((idx&1)as u64)<<15),f[1]|(((idx>>1)as u64)<<15),f[2]].iter().flat_map(|x|(*x as u16).to_le_bytes()).collect(),
        9=>(f[2]|(f[1]<<10)|(f[0]<<20)|((idx as u64)<<30)).to_le_bytes()[..4].to_vec(),
        _=>{let u=f[2]|(f[1]<<20)|(f[0]<<40)|((idx as u64)<<62);[(u>>32)as u32,u as u32].iter().flat_map(|x|x.to_le_bytes()).collect()}
    })
}
fn unit(t:&Track)->usize{match t.k{4|8|12=>4,2|6=>2,_=>1}}
fn encode(m:&Model,v:&Value)->Res<Vec<u8>>{ encode_mode(m,v,false) }
fn encode_mode(m:&Model,v:&Value,import_pn:bool)->Res<Vec<u8>>{
    let original=document(m);
    if v["Format"]!=original["Format"] || v["Name"]!=original["Name"] || v["Duration"]!=original["Duration"] {return Err("format, nom et durée doivent rester ceux du moule".into());}
    let ts=v["Tracks"].as_array().ok_or("Tracks absent")?;if ts.len()!=m.tracks.len(){return Err("nombre de pistes modifié".into());}
    let mut body=vec![];let mut delta=[0i64;3];
    for (idx,(t,j)) in m.tracks.iter().zip(ts).enumerate(){
        if j["TrackIndex"]!=json!(idx)||j["TrackID"]!=json!(m.ids[idx].to_string()){return Err(format!("piste {idx} : ordre ou identifiant modifié"));}
        if j==&original["Tracks"][idx]{body.extend_from_slice(&m.bytes[t.start..t.end]);continue;}
        if !t.editable && !(import_pn && matches!(pn::kind(t),Some(0)|Some(-1))){return Err(format!("piste {idx} : codec opaque, modification interdite"));}
        if j.get("PreserveHex").is_some(){return Err("PreserveHex interdit pour une piste éditable".into());}
        let tag=u8::try_from(number(&j["codec"])?).map_err(|_|"codec trop grand")?;
        let (base,k,mode)=family(tag).ok_or("codec inconnu")?;
        if base!=t.base && !(import_pn && ((pn::kind(t)==Some(0) && matches!(base,6|9|12|15)) || (pn::kind(t)==Some(-1) && base==51))){return Err("changer de famille de codec n'est pas pris en charge".into());}
        let keys=j["Keyframes"].as_array().ok_or("Keyframes absent")?;let n=keys.len();if n==0||n>65536{return Err("nombre de clés invalide".into());}
        let frames:Vec<u32>=keys.iter().map(|k|number(&k["Frame"])).collect::<Res<_>>()?;
        if frames.windows(2).any(|w|w[0]>=w[1])||frames[0]!=0{return Err("temps strictement croissants, première image 0 exigée".into());}
        let limit=((m.duration as f64)*60.).round() as u32+2;
        if *frames.last().unwrap()>limit{return Err("une clé dépasse la durée du moule".into());}
        let mut times=vec![];
        if n>1{match mode{
            0=>for f in &frames[1..]{times.push(u8::try_from(*f).map_err(|_|"temps >255 : utiliser le codec u16 de la même famille")?);},
            1=>for f in &frames[1..]{times.extend_from_slice(&u16::try_from(*f).map_err(|_|"temps >65535")?.to_le_bytes());},
            _=>{if frames.iter().any(|f|f%2!=0){return Err("le masque exige des images paires".into());}let len=(*frames.last().unwrap()as usize/2)/8+1;if len>63{return Err("masque >63 octets".into());}
                // Les bits hauts restent ceux du moule ; leur sens n'est pas établi.
                if t.tag%3!=2 || t.frames.len()==1{return Err("création d'un nouveau format de masque non prise en charge".into());}
                times.push((m.bytes[t.start+9]&0xc0)|len as u8);times.resize(len+1,0);for f in &frames{times[1+(*f as usize/2)/8]|=1<<((*f/2)%8);}}
        }}
        let mut data=vec![];let field=match base{27=>"LocalPosition",51=>"Raw",_=>"LocalRotation"};
        for (i,key) in keys.iter().enumerate(){
            if base==51 {
                let raw=key["Raw"].as_str().ok_or("Raw absent")?;
                if raw.len()!=2 {return Err("Raw doit contenir un octet".into());}
                data.push(u8::from_str_radix(raw,16).map_err(|_|"Raw invalide")?);continue;
            }
            let val=vector(&key[field],if base==27{3}else{4})?;
            // Préserve les bits réservés et la quantification quand la valeur ne change pas.
            if base==t.base && t.editable && i<t.frames.len() && key[field]==original["Tracks"][idx]["Keyframes"][i][field] {data.extend_from_slice(&m.bytes[t.data+i*k..t.data+(i+1)*k]);}
            else if base==27{for x in val{let f=x as f32;if !f.is_finite(){return Err("position hors f32".into());}data.extend_from_slice(&f.to_le_bytes());}}
            else{data.extend_from_slice(&pack_quat(base,val)?);}
        }
        let u=match k{4|8|12=>4,2|6=>2,_=>1};let overhead=if n==1{u.max(2)}else{let extra=match mode{0=>2,1=>3,_=>1};(times.len()+extra+u-1)/u*u};
        let size=data.len()+overhead;
        let slot=match u{4=>0,2=>1,_=>2};delta[slot]+=size as i64/u as i64;let old_unit=unit(t);let old_slot=match old_unit{4=>0,2=>1,_=>2};delta[old_slot]-=t.size as i64/old_unit as i64;
        body.push(tag);body.extend_from_slice(&(size as u32).to_le_bytes());body.extend_from_slice(&(n as u32).to_le_bytes());body.extend(times);body.extend(data);
    }
    let mut out=m.bytes[..m.start].to_vec();out.extend(body);
    let len=out.len();put(&mut out,4,len-m.sub)?;put(&mut out,m.start-8,len-m.start+4)?;
    for (i,d) in delta.iter().enumerate(){let (_,old,pos)=*m.sections.iter().find(|(t,_,_)|*t==[7,5,3][i]).ok_or("compteur absent")?;let new=old as i64+d;if new<0{return Err("compteur négatif".into());}put(&mut out,pos,new as usize)?;}
    let parsed=parse(out.clone())?;if parsed.tracks.len()!=m.tracks.len(){return Err("relecture incohérente".into());}Ok(out)
}
fn read(path:&str)->Res<Vec<u8>>{fs::read(path).map_err(|e|format!("{path}: {e}"))}
fn write_new(path:&str, bytes:&[u8])->Res<()> {
    use std::io::Write;
    let mut f=fs::OpenOptions::new().write(true).create_new(true).open(Path::new(path)).map_err(|e|format!("sortie {path}: {e}"))?;
    f.write_all(bytes).map_err(|e|e.to_string())
}
fn run()->Res<()>{
    let a:Vec<String>=env::args().collect();match a.get(1).map(String::as_str){
        Some("decoder") if a.len()==4=>{let m=parse(read(&a[2])?)?;let bytes=serde_json::to_vec_pretty(&document(&m)).map_err(|e|e.to_string())?;write_new(&a[3],&bytes)?;println!("{} : {} pistes, durée {} s",m.name,m.tracks.len(),m.duration);},
        Some("encoder") if a.len()==5=>{let m=parse(read(&a[2])?)?;let v:Value=serde_json::from_slice(&read(&a[3])?).map_err(|e|e.to_string())?;let b=encode(&m,&v)?;write_new(&a[4],&b)?;println!("{} octets écrits et relus",b.len());},
        Some("importer-pn") if (a.len()==6 || (a.len()==7 && a[6]=="--ignorer-pistes-sans-destination")) && a[5]=="--essai-sans-canaux-globaux"=>{let m=parse(read(&a[2])?)?;let v:Value=serde_json::from_slice(&read(&a[3])?).map_err(|e|e.to_string())?;let (doc,report)=pn::prepare_opt(&m,&v,a.len()==7)?;let b=encode_mode(&m,&doc,true)?;write_new(&a[4],&b)?;println!("{}",serde_json::to_string_pretty(&report).map_err(|e|e.to_string())?);},
        Some("verifier") if a.len()==3=>{let m=parse(read(&a[2])?)?;let b=encode(&m,&document(&m))?;if b!=m.bytes{return Err("aller-retour différent".into());}println!("{} : aller-retour identique, {} pistes",m.name,m.tracks.len());},
        _=>return Err("Usage : animation-encodeur decoder moule.bin sortie.json | encoder moule.bin entree.json sortie.bin | verifier moule.bin | importer-pn moule.bin source-pn.json sortie.bin --essai-sans-canaux-globaux [--ignorer-pistes-sans-destination]".into())
    }Ok(())
}
fn main(){if let Err(e)=run(){eprintln!("ERREUR : {e}");std::process::exit(1);}}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn rotations_aller_retour() {
        for base in [6,9,12,15] {for seed in 0..200 {
            let mut q:Vec<f64>=(0..4).map(|i|((seed*13+i*7+1)as f64).sin()).collect();let norm=q.iter().map(|x|x*x).sum::<f64>().sqrt();for x in &mut q{*x/=norm;}
            let packed=pack_quat(base,q.clone()).unwrap();let got=quat(base,&packed);
            let dot=q.iter().zip(got).map(|(a,b)|a*b).sum::<f64>().abs();
            let bound=match base {6=>0.001,9=>0.00002,12=>0.0000001,_=>0.000000001};assert!(1.-dot<bound,"codec {base} : {dot}");
        }}
    }
    #[test]
    fn donnees_invalides() {
        assert!(pack_quat(9,vec![0.;4]).is_err());
        assert!(vector(&json!([1,2]),3).is_err());
        assert!(number(&json!(-1)).is_err());
        for n in 0..64{assert!(parse(vec![0;n]).is_err());}
    }
    #[test]
    fn temps_et_tailles() {
        let b=[9,12,0,0,0,2,0,0,0,4,0,0,0,0,0,0,0,0];
        let t=track(&b,0).unwrap();assert_eq!(t.frames,vec![0,4]);assert_eq!(t.end,b.len());
        assert!(track(&b[..b.len()-1],0).is_err());
        let b=[11,12,0,0,0,2,0,0,0,0x41,3,0,0,0,0,0,0,0,0];
        assert_eq!(track(&b,0).unwrap().frames,vec![0,2]);
    }
}
