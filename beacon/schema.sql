-- beacon server database specification file

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
