use anyhow::Result;

use crate::graph::{store::GraphStore, GraphQuery};

pub fn promote_bridges_to_calls(store: &GraphStore) -> Result<usize> {
    let _lock = store.write_lock()?;
    let conn = store.connection()?;
    let gq = GraphQuery::new(&conn);

    let query = "MATCH (a:Symbol)-[b:BRIDGE_TO]->(t:Symbol) RETURN a.id, t.id, b.bridge_kind";
    let bridges = gq.raw_query(query)?;

    let mut promoted = 0;
    for row in &bridges {
        if row.len() < 2 {
            continue;
        }
        let source_id = &row[0];
        let target_id = &row[1];

        let check = format!(
            "MATCH (a:Symbol {{id: '{}'}})-[:CALLS]->(b:Symbol {{id: '{}'}}) RETURN a.id",
            source_id.replace('\'', "\\'"),
            target_id.replace('\'', "\\'"),
        );
        let existing = gq.raw_query(&check).unwrap_or_default();
        if !existing.is_empty() {
            continue;
        }

        let insert = format!(
            "MATCH (a:Symbol {{id: '{}'}}), (b:Symbol {{id: '{}'}}) CREATE (a)-[:CALLS]->(b)",
            source_id.replace('\'', "\\'"),
            target_id.replace('\'', "\\'"),
        );
        if gq.raw_query(&insert).is_ok() {
            promoted += 1;
        }
    }
    Ok(promoted)
}
