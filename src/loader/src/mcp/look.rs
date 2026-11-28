use std::collections::{BTreeSet, HashMap, HashSet};

use loader::backend::{self, Settings, Target};
use serde_json::{Map, Value, json};

pub const SECTIONS: [&str; 10] = [
    "player",
    "skills",
    "inventory",
    "equipment",
    "npcs",
    "players",
    "objects",
    "ground_items",
    "interfaces",
    "chat",
];
pub const DEFAULT_SECTIONS: [&str; 3] = ["player", "inventory", "equipment"];
pub const RADIUS: i32 = 15;
pub const LIMIT: usize = 20;
const INVENTORY: i32 = 93;
const EQUIPMENT: i32 = 94;

pub struct Look {
    pub sections: Vec<String>,
    pub name: Option<String>,
    pub radius: i32,
    pub limit: usize,
}

struct Here {
    x: i32,
    y: i32,
    plane: i32,
}

impl Look {
    fn wants(&self, section: &str) -> bool {
        self.sections.iter().any(|s| s == section)
    }

    fn named(&self, name: &str, id: i32) -> bool {
        match &self.name {
            None => true,
            Some(wanted) => match wanted.parse::<i32>() {
                Ok(wanted) => wanted == id,
                Err(_) => name.to_lowercase().contains(wanted),
            },
        }
    }

    fn near(&self, here: &Here, x: i32, y: i32) -> Option<i32> {
        let distance = (x - here.x).abs().max((y - here.y).abs());
        (distance <= self.radius).then_some(distance)
    }
}

fn nearest<T>(mut found: Vec<(i32, T)>, limit: usize) -> Vec<T> {
    found.sort_by_key(|(distance, _)| *distance);
    found
        .into_iter()
        .take(limit)
        .map(|(_, item)| item)
        .collect()
}

pub fn read(target: &Target, settings: &Settings, look: &Look) -> Result<Value, String> {
    let state = backend::state(target, settings)?;
    let here = state.position.as_ref().map(|p| Here {
        x: p.x,
        y: p.y,
        plane: p.plane.unwrap_or(0),
    });
    let mut out = Map::new();
    if look.wants("player") {
        let mut player = serde_json::to_value(&state).map_err(|e| e.to_string())?;
        if let Value::Object(fields) = &mut player {
            fields.remove("skills");
        }
        out.insert("player".into(), player);
    }
    if look.wants("skills") {
        out.insert("skills".into(), json!(state.skills));
    }
    if look.wants("inventory") || look.wants("equipment") {
        items(target, settings, look, &mut out)?;
    }
    let scene = ["npcs", "players", "objects", "ground_items"];
    if scene.iter().any(|section| look.wants(section)) {
        let Some(here) = &here else {
            out.insert(
                "scene".into(),
                json!("The player has no position: not logged in."),
            );
            return Ok(Value::Object(out));
        };
        if look.wants("npcs") {
            out.insert("npcs".into(), npcs(target, settings, look, here)?);
        }
        if look.wants("players") {
            out.insert("players".into(), players(target, settings, look, here)?);
        }
        if look.wants("objects") {
            out.insert("objects".into(), objects(target, settings, look, here)?);
        }
        if look.wants("ground_items") {
            out.insert(
                "ground_items".into(),
                ground_items(target, settings, look, here)?,
            );
        }
    }
    if look.wants("interfaces") {
        let groups: Option<BTreeSet<i32>> = backend::sub_interfaces(target, settings)?
            .map(|open| open.iter().map(|i| i.group).collect());
        out.insert("interfaces".into(), json!(groups));
    }
    if look.wants("chat") {
        out.insert("chat".into(), chat(target, settings, look)?);
    }
    Ok(Value::Object(out))
}

fn items(
    target: &Target,
    settings: &Settings,
    look: &Look,
    out: &mut Map<String, Value>,
) -> Result<(), String> {
    let containers = backend::item_containers(target, settings)?;
    let wanted = |id: i32| {
        containers
            .iter()
            .find(|(container, _)| *container == id)
            .map(|(_, items)| items.clone())
            .unwrap_or_default()
    };
    let (inventory, equipment) = (wanted(INVENTORY), wanted(EQUIPMENT));
    let ids: Vec<i32> = inventory
        .iter()
        .chain(&equipment)
        .map(|item| item.id)
        .collect::<HashSet<_>>()
        .into_iter()
        .collect();
    let names: HashMap<i32, Option<String>> = backend::item_names(target, settings, &ids)?
        .into_iter()
        .zip(&ids)
        .map(|(name, id)| (*id, name))
        .collect();
    let named = |items: &[injector::Item]| -> Vec<Value> {
        items
            .iter()
            .map(|item| {
                json!({
                    "slot": item.slot,
                    "id": item.id,
                    "name": names.get(&item.id).cloned().flatten(),
                    "quantity": item.quantity,
                })
            })
            .collect()
    };
    if look.wants("inventory") {
        out.insert("inventory".into(), json!(named(&inventory)));
    }
    if look.wants("equipment") {
        out.insert("equipment".into(), json!(named(&equipment)));
    }
    Ok(())
}

fn npcs(target: &Target, settings: &Settings, look: &Look, here: &Here) -> Result<Value, String> {
    let found = backend::npcs(target, settings)?
        .into_iter()
        .filter(|npc| look.named(&npc.name, npc.id))
        .filter_map(|npc| look.near(here, npc.x, npc.y).map(|d| (d, (d, npc))))
        .collect();
    let npcs = nearest(found, look.limit);
    let ids: Vec<i32> = npcs
        .iter()
        .map(|(_, npc)| npc.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let actions = if ids.is_empty() {
        Default::default()
    } else {
        backend::npc_actions(target, settings, &ids)?
    };
    Ok(json!(
        npcs.iter()
            .map(|(distance, npc)| json!({
                "name": npc.name,
                "id": npc.id,
                "index": npc.index,
                "x": npc.x,
                "y": npc.y,
                "distance": distance,
                "actions": actions.get(&npc.id),
                "animation": npc.animation,
                "interacting_player": npc.interacting_player,
            }))
            .collect::<Vec<_>>()
    ))
}

fn players(
    target: &Target,
    settings: &Settings,
    look: &Look,
    here: &Here,
) -> Result<Value, String> {
    let found = backend::players(target, settings)?
        .into_iter()
        .filter(|player| look.named(&player.name, player.index))
        .filter_map(|player| {
            look.near(here, player.x, player.y)
                .map(|distance| (distance, (distance, player)))
        })
        .collect();
    Ok(json!(
        nearest(found, look.limit)
            .iter()
            .map(|(distance, player)| json!({
                "name": player.name,
                "index": player.index,
                "x": player.x,
                "y": player.y,
                "distance": distance,
                "animation": player.animation,
                "interacting_npc": player.interacting_npc,
                "interacting_player": player.interacting_player,
            }))
            .collect::<Vec<_>>()
    ))
}

fn objects(
    target: &Target,
    settings: &Settings,
    look: &Look,
    here: &Here,
) -> Result<Value, String> {
    let nearby: Vec<(i32, injector::Loc)> = backend::locs(target, settings)?
        .into_iter()
        .filter(|loc| loc.clickable && loc.plane == here.plane)
        .filter_map(|loc| look.near(here, loc.x, loc.y).map(|d| (d, loc)))
        .collect();
    let ids: Vec<i32> = nearby
        .iter()
        .map(|(_, loc)| loc.id)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    if ids.is_empty() {
        return Ok(json!([]));
    }
    let shown: HashMap<i32, injector::LocView> = ids
        .iter()
        .copied()
        .zip(backend::loc_shown(target, settings, &ids)?)
        .collect();
    let found = nearby
        .into_iter()
        .filter_map(|(distance, loc)| {
            let view = shown.get(&loc.id)?;
            let name = if view.name.is_empty() {
                loc.name.clone()?
            } else {
                view.name.clone()
            };
            (name != "null" && !view.actions.is_empty() && look.named(&name, loc.id))
                .then(|| (distance, (distance, name, view.actions.clone(), loc)))
        })
        .collect();
    Ok(json!(
        nearest(found, look.limit)
            .iter()
            .map(|(distance, name, actions, loc)| json!({
                "name": name,
                "id": loc.id,
                "x": loc.x,
                "y": loc.y,
                "plane": loc.plane,
                "distance": distance,
                "actions": actions,
            }))
            .collect::<Vec<_>>()
    ))
}

fn ground_items(
    target: &Target,
    settings: &Settings,
    look: &Look,
    here: &Here,
) -> Result<Value, String> {
    let found = backend::ground_items(target, settings)?
        .into_iter()
        .filter(|item| item.plane == here.plane)
        .filter(|item| look.named(item.name.as_deref().unwrap_or(""), item.id))
        .filter_map(|item| {
            look.near(here, item.x, item.y)
                .map(|distance| (distance, (distance, item)))
        })
        .collect();
    Ok(json!(
        nearest(found, look.limit)
            .iter()
            .map(|(distance, item)| json!({
                "name": item.name,
                "id": item.id,
                "quantity": item.quantity,
                "x": item.x,
                "y": item.y,
                "distance": distance,
            }))
            .collect::<Vec<_>>()
    ))
}

fn chat(target: &Target, settings: &Settings, look: &Look) -> Result<Value, String> {
    let mut messages: Vec<injector::ChatMessage> = backend::chat_messages(target, settings)?
        .into_iter()
        .filter(|message| {
            look.name.as_ref().is_none_or(|wanted| {
                message.sender.to_lowercase().contains(wanted)
                    || message.text.to_lowercase().contains(wanted)
            })
        })
        .collect();
    messages.sort_by_key(|message| message.time);
    let skip = messages.len().saturating_sub(look.limit);
    Ok(json!(
        messages[skip..]
            .iter()
            .map(|message| json!({
                "kind": message.kind,
                "sender": message.sender,
                "text": message.text,
                "time": message.time_local,
            }))
            .collect::<Vec<_>>()
    ))
}
