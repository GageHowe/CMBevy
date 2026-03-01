-- beacon server database specification file

CREATE TABLE IF NOT EXISTS users (
     id TEXT PRIMARY KEY,
     username TEXT UNIQUE NOT NULL,
     email TEXT UNIQUE,
     password_hash TEXT NOT NULL,
     xp INTEGER DEFAULT 0,
     created_at TIMESTAMP DEFAULT CURRENT_TIMESTAMP
);
