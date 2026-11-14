//! Import strict des pistes de mouvement Puppet Native, sans copier ses références Unity.
use super::*;
use std::collections::{BTreeMap,BTreeSet};

pub(super) fn kind(t:&Track)->Option<i64>{
    match t.base {3|6|9|12|15|21|24=>Some(0),27=>Some(3),51..=53=>Some(-1),69=>Some(5),_=>None}
}
fn tracks(source:&Value)->Res<BTreeMap<(u32,i64),Value>>{
    if source["Additive"].as_i64()!=Some(0){return Err("animation additive ou champ Additive absent".into());}
    if source["Name"].as_str().filter(|n|!n.is_empty()).is_none(){return Err("nom source absent".into());}
    let length=source["Length"].as_f64().ok_or("Length absent")?;
    if !length.is_finite() || length<=0. || length>1092. {return Err("durée source invalide".into());}
    let ts=source["Tracks"].as_array().ok_or("Tracks absent")?;
    if number(&source["NumTracks"])? as usize!=ts.len(){return Err("NumTracks incohérent".into());}
    let mut map=BTreeMap::new();
    let mut ambigues=BTreeSet::new();
    for t in ts{
        let id=t["TrackID"].as_str().ok_or("TrackID doit être une chaîne décimale")?.parse::<u32>().map_err(|_|"TrackID invalide")?;
        let info=t["infotype"].as_i64().ok_or("infotype absent")?;
        let codec=number(&t["codec"])?;
        let keys=t["Keyframes"].as_array().ok_or("Keyframes absent")?;
        if keys.is_empty(){return Err("piste source vide".into());}
        if !matches!(info,0|3){
            // Deux clés sur la même image : seule la dernière compte (c'est la
            // valeur visible à cette image). Les temps qui reculent restent refusés.
            let mut kept:Vec<Value>=Vec::new();
            let mut previous=None;
            for key in keys {
                let frame=number(&key["Frame"])?;
                if previous.is_none() && frame!=0 || previous.map(|p|frame<p).unwrap_or(false) || frame>(length*60.).round() as u32 {return Err("temps de piste spéciale invalide".into());}
                if previous==Some(frame){kept.pop();}
                kept.push(key.clone());
                previous=Some(frame);
                match (info,codec){
                    (-1,39)=>{let raw=key["Raw"].as_str().ok_or("Raw absent")?;if raw.len()!=2||!raw.bytes().all(|x|x.is_ascii_hexdigit()){return Err("Raw invalide".into());}},
                    (5,57)=>{number(&key["Int"])?;},
                    (4,48)=>{},
                    _=>return Err(format!("piste spéciale non reconnue : {info}/{codec}"))
                }
            }
            // Les quatre canaux de contrôle globaux n'ont pas de destination
            // identifiée dans le moule : ils restent signalés, jamais inventés.
            if info==4 || id>=0xffffffe0 {continue;}
            let mut t=t.clone();
            t["Keyframes"]=Value::Array(kept);
            // Plusieurs pistes spéciales avec le même identifiant : impossible de
            // savoir laquelle correspond au moule, donc aucune n'est importée.
            if ambigues.contains(&(id,info)){continue;}
            if map.insert((id,info),t).is_some(){map.remove(&(id,info));ambigues.insert((id,info));}
            continue;
        }
        // Identifiants 0xffffffe0 et plus : canaux spéciaux, pas des os. Le moule
        // garde les siens tels quels ; ceux de la source ne sont pas importés.
        if id>=0xffffffe0 {continue;}
        let family=family(u8::try_from(codec).map_err(|_|"codec hors u8")?).ok_or("codec source inconnu")?.0;
        if (info==3 && family!=27) || (info==0 && !matches!(family,3|6|9|12|15|21|24)){return Err("codec et infotype incompatibles".into());}
        let field=if info==0{"LocalRotation"}else{"LocalPosition"};
        let mut previous=None;
        for key in keys{
            let frame=number(&key["Frame"])?;
            if previous.is_none() && frame!=0 || previous.map(|p|frame<=p).unwrap_or(false){return Err("temps source non croissants ou début différent de zéro".into());}
            if frame>u16::MAX as u32 || frame>(length*60.).round() as u32{return Err("clé hors durée source".into());}
            previous=Some(frame);
            let values=vector(&key[field],if info==0{4}else{3})?;
            if info==0{pack_quat(15,values)?;}else if values.iter().any(|v|!(*v as f32).is_finite()){return Err("position hors f32".into());}
        }
        if map.insert((id,info),t.clone()).is_some(){return Err(format!("piste de mouvement ambiguë pour {id}/{info}"));}
    }
    if map.is_empty(){return Err("aucune piste de mouvement".into());}
    Ok(map)
}

pub(super) fn prepare(m:&Model,source:&Value)->Res<(Value,Value)>{prepare_opt(m,source,false)}

/// `ignorer_sans_destination` : les pistes source absentes du moule sont laissées de
/// côté et listées dans le rapport, au lieu de faire échouer l'import.
pub(super) fn prepare_opt(m:&Model,source:&Value,ignorer_sans_destination:bool)->Res<(Value,Value)>{
    let source_tracks=tracks(source)?;
    let length=source["Length"].as_f64().unwrap();
    if (length-m.duration as f64).abs()>0.00001{return Err(format!("durées différentes : source {length}, moule {} ; aucune accélération ou troncature automatique",m.duration));}
    let mut doc=document(m);let mut used=BTreeSet::new();let mut preserved=vec![];let mut imported=vec![];
    for (idx,t) in m.tracks.iter().enumerate(){
        let Some(info)=kind(t) else {preserved.push(idx);continue;};
        let id=m.ids[idx];let Some(src)=source_tracks.get(&(id,info)) else{preserved.push(idx);continue;};
        if !used.insert((id,info)){return Err(format!("moule ambigu pour {id}/{info}"));}
        let max_frame=src["Keyframes"].as_array().unwrap().last().unwrap()["Frame"].as_u64().unwrap();
        if info==5 {
            let keys=src["Keyframes"].as_array().unwrap();
            if keys.len()!=1 || t.frames.len()!=1 || t.k!=4 || number(&keys[0]["Int"])?!=u32at(&m.bytes,t.data)? {return Err("référence spéciale différente du moule".into());}
            imported.push(json!({"index":idx,"id":id,"infotype":info,"reference_identique":true}));
            continue;
        }
        let codec=(match info{0=>15,3=>27,-1=>51,_=>return Err("type import inconnu".into())})+if max_frame>255{1}else{0};
        let field=match info{0=>"LocalRotation",3=>"LocalPosition",_=>"Raw"};
        let keys:Vec<Value>=src["Keyframes"].as_array().unwrap().iter().map(|k|{let mut value=json!({"Frame":k["Frame"]});value[field]=k[field].clone();value}).collect();
        doc["Tracks"][idx]=json!({"TrackIndex":idx,"TrackID":id.to_string(),"codec":codec,"Keyframes":keys});
        imported.push(json!({"index":idx,"id":id,"infotype":info,"codec_source":src["codec"],"codec_sortie":codec}));
    }
    let missing:Vec<_>=source_tracks.keys().filter(|k|!used.contains(k)).map(|(id,kind)|format!("{id}/{kind}")).collect();
    if !missing.is_empty() && !ignorer_sans_destination {return Err(format!("pistes source sans destination : {}",missing.join(", ")));}
    let report=json!({"pistes_source_ignorees_sans_destination":missing,"source":source["Name"],"moule":m.name,"duree_source":length,"duree_moule":m.duration,"pistes_mouvement_importees":imported.iter().filter(|t|matches!(t["infotype"].as_i64(),Some(0)|Some(3))).count(),"courbes_speciales_importees":imported.iter().filter(|t|t["infotype"].as_i64()==Some(-1)).count(),"references_speciales_verifiees":imported.iter().filter(|t|t["infotype"].as_i64()==Some(5)).count(),"pistes_mouvement":imported,"indices_pistes_moule_conservees":preserved,"pistes_speciales_source_non_transplantees":source["Tracks"].as_array().unwrap().len()-source_tracks.len(),"canaux_globaux_non_convertis":source["Tracks"].as_array().unwrap().iter().filter(|t| t["infotype"].as_i64()==Some(4)||t["TrackID"].as_str()==Some("4294967264")).cloned().collect::<Vec<_>>(),"installation_autorisee_par_validation":false,"retargeting_bind_pose":false,"statut":"conversion hors jeu ; événements du moule conservés, chargement moteur non validé"});
    Ok((doc,report))
}

#[cfg(test)]
mod tests{
    use super::*;
    fn source()->Value{json!({"Name":"test","Length":1.0,"Additive":0,"NumTracks":1,"Tracks":[{"TrackID":"7","codec":15,"infotype":0,"Keyframes":[{"Frame":0,"LocalRotation":[0.,0.,0.,1.]},{"Frame":60,"LocalRotation":[0.,0.,0.,1.]}]}]})}
    #[test]fn identifiants_et_temps(){let v=source();assert_eq!(tracks(&v).unwrap().len(),1);let mut w=v.clone();w["Tracks"][0]["Keyframes"][1]["Frame"]=json!(61);assert!(tracks(&w).is_err());w=v.clone();w["Tracks"][0]["Keyframes"][1]["Frame"]=json!(0);assert!(tracks(&w).is_err());w=v.clone();w["Tracks"].as_array_mut().unwrap().push(v["Tracks"][0].clone());w["NumTracks"]=json!(2);assert!(tracks(&w).is_err());}
    #[test]fn refus_sources_invalides(){let v=source();for field in ["Name","Additive","NumTracks"]{let mut w=v.clone();w.as_object_mut().unwrap().remove(field);assert!(tracks(&w).is_err());}let mut w=v.clone();w["Tracks"][0]["codec"]=json!(27);assert!(tracks(&w).is_err());w=v.clone();w["Tracks"][0]["Keyframes"][0]["LocalRotation"]=json!([0.,0.,0.,0.]);assert!(tracks(&w).is_err());}
    #[test]fn courbes_speciales_animees(){
        let mut v=source();v["Tracks"].as_array_mut().unwrap().push(json!({"TrackID":"7","codec":39,"infotype":-1,"Keyframes":[{"Frame":0,"Raw":"7d"},{"Frame":30,"Raw":"00"},{"Frame":60,"Raw":"7d"}]}));
        v["Tracks"].as_array_mut().unwrap().push(json!({"TrackID":"1","codec":48,"infotype":4,"Keyframes":[{"Frame":0},{"Frame":60}]}));v["NumTracks"]=json!(3);
        assert_eq!(tracks(&v).unwrap().len(),2);
        v["Tracks"][1]["Keyframes"][1]["Raw"]=json!("zz");assert!(tracks(&v).is_err());
    }
}
