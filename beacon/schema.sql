-- beacon server database specification file

CREATE TABLE IF NOT EXISTS accounts (
     id TEXT PRIMARY KEY,
     display_name TEXT NOT NULL,
     created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS account_identities (
     account_id TEXT NOT NULL,
     provider TEXT NOT NULL,
     provider_user_id TEXT NOT NULL,
     created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
     PRIMARY KEY (provider, provider_user_id),
     FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS account_identities_account_id_idx ON account_identities (account_id);

CREATE TABLE IF NOT EXISTS standalone_credentials (
     account_id TEXT PRIMARY KEY,
     email TEXT UNIQUE NOT NULL,
     password_salt TEXT NOT NULL,
     password_hash TEXT NOT NULL,
     created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
     FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS auth_sessions (
     token TEXT PRIMARY KEY,
     account_id TEXT NOT NULL,
     created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
     FOREIGN KEY (account_id) REFERENCES accounts(id) ON DELETE CASCADE
);

CREATE TABLE IF NOT EXISTS users (
     id TEXT PRIMARY KEY,
     username TEXT UNIQUE NOT NULL,
     email TEXT UNIQUE,
     password_hash TEXT NOT NULL,
     xp INTEGER DEFAULT 0,
     created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS assets (
     hash TEXT PRIMARY KEY,
     file_name TEXT NOT NULL,
     size_bytes INTEGER NOT NULL,
     created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
     updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);

CREATE TABLE IF NOT EXISTS asset_votes (
     asset_hash TEXT NOT NULL,
     ip TEXT NOT NULL,
     vote INTEGER NOT NULL CHECK (vote IN (0, 1)),
     updated_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP,
     PRIMARY KEY (asset_hash, ip),
     FOREIGN KEY (asset_hash) REFERENCES assets(hash) ON DELETE CASCADE
);

CREATE INDEX IF NOT EXISTS asset_votes_asset_hash_idx ON asset_votes (asset_hash);
