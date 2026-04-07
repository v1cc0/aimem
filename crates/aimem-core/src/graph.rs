//! AiMem graph — traverse rooms and wings in the memory store.

use std::collections::{HashMap, HashSet, VecDeque};

use thiserror::Error;

use crate::{
    db::{AimemDb, DbError},
    types::{RoomNode, Tunnel},
};

#[derive(Debug, Error)]
pub enum GraphError {
    #[error("db: {0}")]
    Db(#[from] DbError),
    #[error("turso: {0}")]
    Turso(#[from] turso::Error),
}

/// AiMem graph traversal.
#[derive(Debug, Clone)]
pub struct AimemGraph {
    db: AimemDb,
}

impl AimemGraph {
    pub fn new(db: AimemDb) -> Self {
        Self { db }
    }

    /// Build full room→wings mapping from the memory store.
    pub async fn build(&self) -> Result<(Vec<RoomNode>, Vec<Tunnel>), GraphError> {
        let conn = self.db.conn()?;
        let mut room_wings: HashMap<String, (HashSet<String>, i64)> = HashMap::new();

        let mut rows = conn
            .query(
                "SELECT room, wing, COUNT(*) as cnt \
                 FROM drawers \
                 WHERE room != 'general' \
                 GROUP BY room, wing",
                (),
            )
            .await?;

        while let Some(row) = rows.next().await? {
            let room = val_str(&row, 0);
            let wing = val_str(&row, 1);
            let cnt = row
                .get_value(2)
                .ok()
                .and_then(|v| v.as_integer().copied())
                .unwrap_or(0);
            if room.is_empty() || wing.is_empty() {
                continue;
            }
            let entry = room_wings.entry(room).or_insert((HashSet::new(), 0));
            entry.0.insert(wing);
            entry.1 += cnt;
        }

        let mut nodes: Vec<RoomNode> = room_wings
            .iter()
            .map(|(room, (wings, cnt))| {
                let mut ws: Vec<String> = wings.iter().cloned().collect();
                ws.sort();
                RoomNode {
                    room: room.clone(),
                    wings: ws,
                    drawer_count: *cnt,
                }
            })
            .collect();
        nodes.sort_by(|a, b| a.room.cmp(&b.room));

        let tunnels: Vec<Tunnel> = nodes
            .iter()
            .filter(|n| n.wings.len() >= 2)
            .map(|n| Tunnel {
                room: n.room.clone(),
                wings: n.wings.clone(),
                drawer_count: n.drawer_count,
            })
            .collect();

        Ok((nodes, tunnels))
    }

    /// BFS from a starting room, up to `max_hops`.
    pub async fn traverse(
        &self,
        start_room: &str,
        max_hops: usize,
    ) -> Result<Vec<TraversalNode>, GraphError> {
        let (nodes, _) = self.build().await?;
        let node_map: HashMap<&str, &RoomNode> =
            nodes.iter().map(|n| (n.room.as_str(), n)).collect();

        let Some(start) = node_map.get(start_room) else {
            let suggestions: Vec<String> = nodes
                .iter()
                .filter(|n| n.room.contains(start_room) || start_room.contains(n.room.as_str()))
                .take(5)
                .map(|n| n.room.clone())
                .collect();
            return Ok(vec![TraversalNode {
                room: format!("Room '{start_room}' not found. Suggestions: {suggestions:?}"),
                wings: vec![],
                drawer_count: 0,
                hop: 0,
                connected_via: vec![],
            }]);
        };

        let mut visited: HashSet<&str> = HashSet::new();
        visited.insert(start_room);

        let mut results = vec![TraversalNode {
            room: start.room.clone(),
            wings: start.wings.clone(),
            drawer_count: start.drawer_count,
            hop: 0,
            connected_via: vec![],
        }];

        let mut frontier: VecDeque<(&str, usize)> = VecDeque::new();
        frontier.push_back((start_room, 0));

        while let Some((current_room, depth)) = frontier.pop_front() {
            if depth >= max_hops {
                continue;
            }
            let current_wings: HashSet<&str> = node_map
                .get(current_room)
                .map(|n| n.wings.iter().map(|w| w.as_str()).collect())
                .unwrap_or_default();

            for (room, node) in &node_map {
                if visited.contains(room) {
                    continue;
                }
                let shared: Vec<String> = node
                    .wings
                    .iter()
                    .filter(|w| current_wings.contains(w.as_str()))
                    .cloned()
                    .collect();
                if !shared.is_empty() {
                    visited.insert(room);
                    results.push(TraversalNode {
                        room: node.room.clone(),
                        wings: node.wings.clone(),
                        drawer_count: node.drawer_count,
                        hop: depth + 1,
                        connected_via: shared,
                    });
                    if depth + 1 < max_hops {
                        frontier.push_back((room, depth + 1));
                    }
                }
            }
        }

        results.sort_by(|a, b| a.hop.cmp(&b.hop).then(b.drawer_count.cmp(&a.drawer_count)));
        results.truncate(50);
        Ok(results)
    }

    /// Find rooms that bridge two wings.
    pub async fn find_tunnels(
        &self,
        wing_a: Option<&str>,
        wing_b: Option<&str>,
    ) -> Result<Vec<Tunnel>, GraphError> {
        let (_, mut tunnels) = self.build().await?;
        if let Some(wa) = wing_a {
            tunnels.retain(|t| t.wings.contains(&wa.to_string()));
        }
        if let Some(wb) = wing_b {
            tunnels.retain(|t| t.wings.contains(&wb.to_string()));
        }
        tunnels.sort_by(|a, b| b.drawer_count.cmp(&a.drawer_count));
        tunnels.truncate(50);
        Ok(tunnels)
    }

    /// Graph summary statistics.
    pub async fn stats(&self) -> Result<serde_json::Value, GraphError> {
        let (nodes, tunnels) = self.build().await?;
        let mut rooms_per_wing: HashMap<String, usize> = HashMap::new();
        for node in &nodes {
            for wing in &node.wings {
                *rooms_per_wing.entry(wing.clone()).or_default() += 1;
            }
        }
        let mut rpw: Vec<(String, usize)> = rooms_per_wing.into_iter().collect();
        rpw.sort_by(|a, b| b.1.cmp(&a.1));

        Ok(serde_json::json!({
            "total_rooms": nodes.len(),
            "tunnel_rooms": tunnels.len(),
            "rooms_per_wing": rpw.into_iter().map(|(w, c)| serde_json::json!({"wing": w, "count": c})).collect::<Vec<_>>(),
            "top_tunnels": tunnels.iter().take(10).map(|t| serde_json::json!({
                "room": t.room, "wings": t.wings, "count": t.drawer_count,
            })).collect::<Vec<_>>(),
        }))
    }
}

#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
pub struct TraversalNode {
    pub room: String,
    pub wings: Vec<String>,
    pub drawer_count: i64,
    pub hop: usize,
    pub connected_via: Vec<String>,
}

fn val_str(row: &turso::Row, idx: usize) -> String {
    match row.get_value(idx) {
        Ok(turso::Value::Text(s)) => s,
        Ok(turso::Value::Null) | Err(_) => String::new(),
        Ok(v) => format!("{v:?}"),
    }
}
