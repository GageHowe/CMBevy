use std::{
    path::Path,
    sync::{Arc, Mutex},
};

use http_common::{
    AccountIdentity, AccountProfile, AuthSessionResponse, IdentityProvider, ProviderLinkRequest,
    ProviderLoginRequest, StandaloneLoginRequest, StandaloneRegisterRequest,
};
use rusqlite::{Connection, params};
use sha2::{Digest, Sha256};

use crate::auth::ApiError;

pub(crate) type Db = Arc<Mutex<Connection>>;

pub(crate) struct AssetRow {
    pub(crate) hash: String,
    pub(crate) file_name: String,
    pub(crate) size_bytes: u64,
    pub(crate) upvotes: u64,
    pub(crate) downvotes: u64,
}

pub(crate) fn open(path: &str) -> Db {
    let conn = Connection::open(path).unwrap();
    conn.pragma_update(None, "foreign_keys", "ON").unwrap();
    conn.execute_batch(include_str!("../schema.sql")).unwrap();
    Arc::new(Mutex::new(conn))
}

pub(crate) fn create_standalone_account(
    db: &Db,
    req: &StandaloneRegisterRequest,
) -> Result<AuthSessionResponse, ApiError> {
    let email = normalize_email(&req.email).ok_or(ApiError::bad_request("invalid email"))?;
    let display_name =
        normalize_display_name(&req.display_name).ok_or(ApiError::bad_request("invalid display name"))?;
    let password = normalize_password(&req.password).ok_or(ApiError::bad_request("invalid password"))?;
    let account_id = next_id("acct");
    let salt = next_id("salt");
    let password_hash = hash_password(&salt, password);
    let token = next_id("sess");
    let mut conn = db.lock().unwrap();
    let tx = conn
        .transaction()
        .map_err(|_| ApiError::internal("failed to open transaction"))?;
    let inserted = tx
        .execute(
            "
            INSERT INTO accounts (id, display_name)
            VALUES (?1, ?2)
            ",
            params![account_id, display_name],
        )
        .map_err(sqlite_auth_error)?;
    if inserted != 1 {
        return Err(ApiError::internal("failed to create account"));
    }
    tx.execute(
        "
        INSERT INTO account_identities (account_id, provider, provider_user_id)
        VALUES (?1, ?2, ?3)
        ",
        params![account_id, IdentityProvider::Standalone.as_str(), email],
    )
    .map_err(sqlite_auth_error)?;
    tx.execute(
        "
        INSERT INTO standalone_credentials (account_id, email, password_salt, password_hash)
        VALUES (?1, ?2, ?3, ?4)
        ",
        params![account_id, email, salt, password_hash],
    )
    .map_err(sqlite_auth_error)?;
    tx.execute(
        "
        INSERT INTO auth_sessions (token, account_id)
        VALUES (?1, ?2)
        ",
        params![token, account_id],
    )
    .map_err(|_| ApiError::internal("failed to create session"))?;
    tx.commit()
        .map_err(|_| ApiError::internal("failed to commit transaction"))?;
    session_by_token_locked(&conn, &token)
}

pub(crate) fn login_standalone_account(
    db: &Db,
    req: &StandaloneLoginRequest,
) -> Result<AuthSessionResponse, ApiError> {
    let email = normalize_email(&req.email).ok_or(ApiError::bad_request("invalid email"))?;
    let password = normalize_password(&req.password).ok_or(ApiError::bad_request("invalid password"))?;
    let conn = db.lock().unwrap();
    let mut stmt = conn
        .prepare(
            "
            SELECT account_id, password_salt, password_hash
            FROM standalone_credentials
            WHERE email = ?1
            ",
        )
        .map_err(|_| ApiError::internal("failed to prepare login query"))?;
    let (account_id, salt, expected): (String, String, String) = stmt
        .query_row([email], |row| Ok((row.get(0)?, row.get(1)?, row.get(2)?)))
        .map_err(|_| ApiError::unauthorized("invalid login"))?;
    if hash_password(&salt, password) != expected {
        return Err(ApiError::unauthorized("invalid login"));
    }
    let token = next_id("sess");
    conn.execute(
        "
        INSERT INTO auth_sessions (token, account_id)
        VALUES (?1, ?2)
        ",
        params![token, account_id],
    )
    .map_err(|_| ApiError::internal("failed to create session"))?;
    session_by_token_locked(&conn, &token)
}

pub(crate) fn login_provider_account(
    db: &Db,
    req: &ProviderLoginRequest,
) -> Result<AuthSessionResponse, ApiError> {
    if req.provider == IdentityProvider::Standalone {
        return Err(ApiError::bad_request("use standalone login for standalone accounts"));
    }
    let provider_user_id = normalize_provider_user_id(&req.provider_user_id)
        .ok_or(ApiError::bad_request("invalid provider user id"))?;
    let display_name =
        normalize_display_name(&req.display_name).ok_or(ApiError::bad_request("invalid display name"))?;
    let token = next_id("sess");
    let mut conn = db.lock().unwrap();
    let tx = conn
        .transaction()
        .map_err(|_| ApiError::internal("failed to open transaction"))?;
    let account_id = find_account_id_by_identity_tx(&tx, req.provider, &provider_user_id)?
        .unwrap_or_else(|| next_id("acct"));
    if !account_exists_tx(&tx, &account_id)? {
        tx.execute(
            "
            INSERT INTO accounts (id, display_name)
            VALUES (?1, ?2)
            ",
            params![account_id, display_name],
        )
        .map_err(|_| ApiError::internal("failed to create account"))?;
        tx.execute(
            "
            INSERT INTO account_identities (account_id, provider, provider_user_id)
            VALUES (?1, ?2, ?3)
            ",
            params![account_id, req.provider.as_str(), provider_user_id],
        )
        .map_err(sqlite_auth_error)?;
    }
    tx.execute(
        "
        INSERT INTO auth_sessions (token, account_id)
        VALUES (?1, ?2)
        ",
        params![token, account_id],
    )
    .map_err(|_| ApiError::internal("failed to create session"))?;
    tx.commit()
        .map_err(|_| ApiError::internal("failed to commit transaction"))?;
    session_by_token_locked(&conn, &token)
}

pub(crate) fn link_provider_identity(
    db: &Db,
    req: &ProviderLinkRequest,
) -> Result<AuthSessionResponse, ApiError> {
    if req.provider == IdentityProvider::Standalone {
        return Err(ApiError::bad_request("standalone identity is linked through standalone registration"));
    }
    let provider_user_id = normalize_provider_user_id(&req.provider_user_id)
        .ok_or(ApiError::bad_request("invalid provider user id"))?;
    let mut conn = db.lock().unwrap();
    let tx = conn
        .transaction()
        .map_err(|_| ApiError::internal("failed to open transaction"))?;
    let account_id = session_account_id_tx(&tx, &req.session_token)?;
    if let Some(existing) = find_account_id_by_identity_tx(&tx, req.provider, &provider_user_id)? {
        if existing != account_id {
            return Err(ApiError::conflict("identity already linked to another account"));
        }
    } else {
        tx.execute(
            "
            INSERT INTO account_identities (account_id, provider, provider_user_id)
            VALUES (?1, ?2, ?3)
            ",
            params![account_id, req.provider.as_str(), provider_user_id],
        )
        .map_err(sqlite_auth_error)?;
    }
    tx.commit()
        .map_err(|_| ApiError::internal("failed to commit transaction"))?;
    session_by_token_locked(&conn, &req.session_token)
}

pub(crate) fn delete_session(db: &Db, token: &str) -> Result<(), ApiError> {
    let conn = db.lock().unwrap();
    conn.execute("DELETE FROM auth_sessions WHERE token = ?1", [token])
        .map_err(|_| ApiError::internal("failed to delete session"))?;
    Ok(())
}

pub(crate) fn session_by_token(db: &Db, token: &str) -> Result<AuthSessionResponse, ApiError> {
    let conn = db.lock().unwrap();
    session_by_token_locked(&conn, token)
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

fn session_by_token_locked(conn: &Connection, token: &str) -> Result<AuthSessionResponse, ApiError> {
    let account_id = conn
        .query_row(
            "
            SELECT account_id
            FROM auth_sessions
            WHERE token = ?1
            ",
            [token],
            |row| row.get::<_, String>(0),
        )
        .map_err(|_| ApiError::unauthorized("invalid session"))?;
    Ok(AuthSessionResponse {
        session_token: token.to_string(),
        account: account_profile(conn, &account_id)?,
    })
}

fn account_profile(conn: &Connection, account_id: &str) -> Result<AccountProfile, ApiError> {
    let display_name = conn
        .query_row(
            "
            SELECT display_name
            FROM accounts
            WHERE id = ?1
            ",
            [account_id],
            |row| row.get::<_, String>(0),
        )
        .map_err(|_| ApiError::unauthorized("missing account"))?;
    let mut stmt = conn
        .prepare(
            "
            SELECT provider, provider_user_id
            FROM account_identities
            WHERE account_id = ?1
            ORDER BY created_at ASC
            ",
        )
        .map_err(|_| ApiError::internal("failed to prepare account query"))?;
    let identities = stmt
        .query_map([account_id], |row| {
            let provider = parse_provider(&row.get::<_, String>(0)?).ok_or(rusqlite::Error::InvalidQuery)?;
            Ok(AccountIdentity {
                provider,
                provider_user_id: row.get(1)?,
            })
        })
        .map_err(|_| ApiError::internal("failed to query account identities"))?
        .flatten()
        .collect();
    Ok(AccountProfile {
        account_id: account_id.to_string(),
        display_name,
        identities,
    })
}

fn account_exists_tx(tx: &rusqlite::Transaction<'_>, account_id: &str) -> Result<bool, ApiError> {
    let exists = tx
        .query_row(
            "SELECT 1 FROM accounts WHERE id = ?1",
            [account_id],
            |_| Ok(()),
        )
        .is_ok();
    Ok(exists)
}

fn find_account_id_by_identity_tx(
    tx: &rusqlite::Transaction<'_>,
    provider: IdentityProvider,
    provider_user_id: &str,
) -> Result<Option<String>, ApiError> {
    match tx.query_row(
        "
        SELECT account_id
        FROM account_identities
        WHERE provider = ?1 AND provider_user_id = ?2
        ",
        params![provider.as_str(), provider_user_id],
        |row| row.get::<_, String>(0),
    ) {
        Ok(account_id) => Ok(Some(account_id)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(_) => Err(ApiError::internal("failed to query account identity")),
    }
}

fn session_account_id_tx(
    tx: &rusqlite::Transaction<'_>,
    token: &str,
) -> Result<String, ApiError> {
    tx.query_row(
        "
        SELECT account_id
        FROM auth_sessions
        WHERE token = ?1
        ",
        [token],
        |row| row.get::<_, String>(0),
    )
    .map_err(|_| ApiError::unauthorized("invalid session"))
}

fn sqlite_auth_error(err: rusqlite::Error) -> ApiError {
    if let rusqlite::Error::SqliteFailure(code, _) = &err {
        if code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_UNIQUE
            || code.extended_code == rusqlite::ffi::SQLITE_CONSTRAINT_PRIMARYKEY
        {
            return ApiError::conflict("identity already exists");
        }
    }
    ApiError::internal("database error")
}

fn normalize_email(value: &str) -> Option<String> {
    let value = value.trim().to_ascii_lowercase();
    (value.contains('@') && value.len() <= 320).then_some(value)
}

fn normalize_display_name(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= 32).then_some(value.to_string())
}

fn normalize_password(value: &str) -> Option<&str> {
    let value = value.trim();
    (value.len() >= 8 && value.len() <= 256).then_some(value)
}

fn normalize_provider_user_id(value: &str) -> Option<String> {
    let value = value.trim();
    (!value.is_empty() && value.len() <= 128).then_some(value.to_string())
}

fn parse_provider(value: &str) -> Option<IdentityProvider> {
    match value {
        "steam" => Some(IdentityProvider::Steam),
        "xbox" => Some(IdentityProvider::Xbox),
        "standalone" => Some(IdentityProvider::Standalone),
        _ => None,
    }
}

fn next_id(prefix: &str) -> String {
    format!("{prefix}_{}", random_hex(24))
}

fn random_hex(len: usize) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut out = String::with_capacity(len);
    for _ in 0..len {
        out.push(HEX[fastrand::usize(..HEX.len())] as char);
    }
    out
}

fn hash_password(salt: &str, password: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(salt.as_bytes());
    hasher.update([0]);
    hasher.update(password.as_bytes());
    let digest = hasher.finalize();
    let mut out = String::with_capacity(digest.len() * 2);
    for byte in digest {
        use std::fmt::Write as _;
        let _ = write!(&mut out, "{byte:02x}");
    }
    out
}
