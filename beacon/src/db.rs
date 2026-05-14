use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use rusqlite::{Connection, params};

pub(crate) type Db = Arc<Mutex<Connection>>;

pub(crate) struct AssetRow {
    pub(crate) hash: String,
    pub(crate) file_name: String,
    pub(crate) size_bytes: u64,
    pub(crate) upvotes: u64,
    pub(crate) downvotes: u64,
}

pub(crate) fn open(path: &Path) -> Db {
    if let Some(dir) = parent_dir(path) {
        std::fs::create_dir_all(dir).unwrap();
    }
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    conn.execute_batch(include_str!("../schema.sql")).unwrap();
    Arc::new(Mutex::new(conn))
}

fn parent_dir(path: &Path) -> Option<&Path> {
    let dir = path.parent()?;
    (!dir.as_os_str().is_empty()).then_some(dir)
}

pub(crate) fn sync_assets(db: &Db, dir: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() || path.extension().is_some_and(|ext| ext == "name") {
            continue;
        }
        let Ok(meta) = entry.metadata() else {
            continue;
        };
        let hash = entry.file_name().to_string_lossy().into_owned();
        let file_name = std::fs::read_to_string(dir.join(format!("{hash}.name")))
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty())
            .unwrap_or_else(|| hash.clone());
        upsert_asset(db, &hash, &file_name, meta.len());
    }
}

pub(crate) fn upsert_asset(db: &Db, hash: &str, file_name: &str, size_bytes: u64) {
    let conn = db.lock().unwrap();
    let _ = conn.execute(
        "
        INSERT INTO assets (hash, file_name, size_bytes)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(hash) DO UPDATE SET
            file_name = excluded.file_name,
            size_bytes = excluded.size_bytes,
            updated_at = CURRENT_TIMESTAMP
        ",
        params![hash, file_name, size_bytes as i64],
    );
}

pub(crate) fn vote(db: &Db, hash: &str, ip: &str, vote: bool) {
    let conn = db.lock().unwrap();
    let _ = conn.execute(
        "
        INSERT INTO asset_votes (asset_hash, ip, vote)
        VALUES (?1, ?2, ?3)
        ON CONFLICT(asset_hash, ip) DO UPDATE SET
            vote = excluded.vote,
            updated_at = CURRENT_TIMESTAMP
        ",
        params![hash, ip, if vote { 1 } else { 0 }],
    );
}

pub(crate) fn list_assets(db: &Db, query: Option<&str>) -> Vec<AssetRow> {
    let filter = query
        .map(|value| format!("%{}%", value.trim().to_ascii_lowercase()))
        .filter(|value| value != "%%");
    let conn = db.lock().unwrap();
    let mut stmt = conn
        .prepare(
            "
            SELECT
                a.hash,
                a.file_name,
                a.size_bytes,
                COALESCE(SUM(CASE WHEN v.vote = 1 THEN 1 ELSE 0 END), 0) AS upvotes,
                COALESCE(SUM(CASE WHEN v.vote = 0 THEN 1 ELSE 0 END), 0) AS downvotes
            FROM assets a
            LEFT JOIN asset_votes v ON v.asset_hash = a.hash
            WHERE (?1 IS NULL OR lower(a.file_name) LIKE ?1 OR lower(a.hash) LIKE ?1)
            GROUP BY a.hash, a.file_name, a.size_bytes, a.updated_at
            ORDER BY
                CASE
                    WHEN COUNT(v.ip) >= 3 THEN CAST(SUM(CASE WHEN v.vote = 1 THEN 1 ELSE 0 END) AS REAL) / COUNT(v.ip)
                    ELSE -1
                END DESC,
                COUNT(v.ip) DESC,
                a.updated_at DESC
            ",
        )
        .unwrap();
    stmt.query_map([filter.as_deref()], |row| {
        Ok(AssetRow {
            hash: row.get(0)?,
            file_name: row.get(1)?,
            size_bytes: row.get::<_, i64>(2)? as u64,
            upvotes: row.get::<_, i64>(3)? as u64,
            downvotes: row.get::<_, i64>(4)? as u64,
        })
    })
    .unwrap()
    .flatten()
    .collect()
}
