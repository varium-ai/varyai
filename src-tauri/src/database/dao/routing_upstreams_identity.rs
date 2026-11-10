//! Public provider IDs can change without changing immutable forwarding identity.
use super::{read_records, CONFLICT, FAILED, INVALID};
use crate::services::routing_upstreams::{valid_identity, RoutingUpstream};
use rusqlite::{params, Connection, OptionalExtension};
use std::collections::BTreeMap;

const KEY: &str = "provider_identities";

pub(super) fn read(conn: &Connection) -> Result<BTreeMap<String, String>, String> {
    let text: Option<String> = conn
        .query_row(
            "SELECT value FROM intent_routing_preferences WHERE key=?1",
            [KEY],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| FAILED)?;
    let identities: BTreeMap<String, String> = text
        .map(|text| {
            if text.len() > 2 * 1024 * 1024 {
                return Err(FAILED.to_owned());
            }
            serde_json::from_str(&text).map_err(|_| FAILED.to_owned())
        })
        .transpose()?
        .unwrap_or_default();
    if identities.len() > 16384
        || identities
            .iter()
            .any(|(id, identity)| !valid_identity(id) || !valid_identity(identity))
    {
        return Err(FAILED.into());
    }
    Ok(identities)
}

pub(super) fn update(
    conn: &Connection,
    current: Option<&RoutingUpstream>,
    next: &RoutingUpstream,
) -> Result<(), String> {
    if current.is_some_and(|old| old.id == next.id) {
        return Ok(());
    }
    let mut identities = read(conn)?;
    if let Some(old) = current {
        let identity = identities
            .get(&old.id)
            .cloned()
            .unwrap_or_else(|| old.id.clone());
        identities.insert(old.id.clone(), identity.clone());
        identities.insert(next.id.clone(), identity);
        migrate_references(conn, &old.id, &next.id)?;
    } else if identities.contains_key(&next.id) {
        // Reusing a public ID creates a new owner, never the retired owner's aliases.
        let records = read_records(conn)?;
        let identity = loop {
            let candidate = format!("provider-{}", uuid::Uuid::new_v4().simple());
            if !identities.values().any(|id| id == &candidate)
                && !records.iter().any(|r| r.id == candidate)
            {
                break candidate;
            }
        };
        identities.insert(next.id.clone(), identity);
    } else {
        return Ok(());
    }
    if identities.len() > 16384 {
        return Err(INVALID.into());
    }
    store(conn, &identities)
}

pub(super) fn retire(conn: &Connection, record: &RoutingUpstream) -> Result<(), String> {
    let mut identities = read(conn)?;
    identities
        .entry(record.id.clone())
        .or_insert_with(|| record.id.clone());
    store(conn, &identities)
}

fn store(conn: &Connection, identities: &BTreeMap<String, String>) -> Result<(), String> {
    let value = serde_json::to_string(identities).map_err(|_| FAILED)?;
    if identities.len() > 16384 || value.len() > 2 * 1024 * 1024 {
        return Err(INVALID.into());
    }
    conn.execute("INSERT INTO intent_routing_preferences(key,value) VALUES (?1,?2) ON CONFLICT(key) DO UPDATE SET value=excluded.value", params![KEY, value]).map_err(|_| FAILED)?;
    Ok(())
}

pub(super) fn migrate_references(conn: &Connection, old: &str, new: &str) -> Result<(), String> {
    let groups = {
        let mut stmt = conn
            .prepare("SELECT config FROM intent_routing_groups LIMIT 33")
            .map_err(|_| FAILED)?;
        let rows = stmt
            .query_map([], |r| r.get::<_, String>(0))
            .map_err(|_| FAILED)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(|_| FAILED)?;
        if rows.len() > 32 {
            return Err(FAILED.into());
        }
        rows
    };
    for text in groups {
        let mut group: crate::services::intent_routing::RoutingGroup =
            serde_json::from_str(&text).map_err(|_| FAILED)?;
        let mut changed = false;
        for member in &mut group.members {
            if member.provider_id == old {
                member.provider_id = new.into();
                changed = true;
            }
        }
        if let Some(target) = &mut group.classifier_target {
            if target.provider_id == old {
                target.provider_id = new.into();
                changed = true;
            }
        }
        if changed {
            group.revision = group.revision.checked_add(1).ok_or(CONFLICT)?;
            group.validate().map_err(|_| FAILED)?;
            conn.execute(
                "UPDATE intent_routing_groups SET revision=?1,config=?2 WHERE id=?3",
                params![
                    group.revision,
                    serde_json::to_string(&group).map_err(|_| FAILED)?,
                    group.id
                ],
            )
            .map_err(|_| FAILED)?;
        }
    }
    let skipped: Option<String> = conn
        .query_row(
            "SELECT value FROM intent_routing_preferences WHERE key='auto_group_skip'",
            [],
            |r| r.get(0),
        )
        .optional()
        .map_err(|_| FAILED)?;
    if let Some(text) = skipped {
        let mut ids: Vec<String> = serde_json::from_str(&text).map_err(|_| FAILED)?;
        for id in &mut ids {
            if id == old {
                *id = new.into();
            }
        }
        conn.execute(
            "UPDATE intent_routing_preferences SET value=?1 WHERE key='auto_group_skip'",
            [serde_json::to_string(&ids).map_err(|_| FAILED)?],
        )
        .map_err(|_| FAILED)?;
    }
    Ok(())
}
