BEGIN;

-- Normalized records from all searchable sources: apps, files, folders,
-- commands, URLs, settings, and future plugin actions.
CREATE TABLE IF NOT EXISTS search_entries (
    -- Internal SQLite row id. Do not use this as a stable identity across rescans.
    id                INTEGER PRIMARY KEY,

    -- Scanner that produced this row. Examples: macos_apps, windows_start_menu,
    -- linux_desktop_entries, filesystem_home, builtin_actions, plugin:<name>.
    source            TEXT NOT NULL,

    -- Current scan generation for this source. When a source is rescanned, rows
    -- not touched by the latest generation can be removed as stale.
    source_generation INTEGER NOT NULL DEFAULT 0,

    -- What the user is selecting. This decides icons, ranking boosts, and which
    -- launch behavior is valid.
    kind              TEXT NOT NULL CHECK (
        kind IN ('app', 'file', 'folder', 'command', 'url', 'setting', 'action')
    ),

    -- Durable unique id for upserts and usage history. It should stay the same
    -- across rescans. Examples: app bundle id, desktop file id, normalized path.
    stable_key        TEXT NOT NULL UNIQUE,

    -- Primary visible label in search results, such as "Visual Studio Code".
    title             TEXT NOT NULL,

    -- Secondary visible label, such as an app category, folder, or description.
    subtitle          TEXT,

    -- Real file path when the entry has one. Commands, settings, and URLs may not.
    path              TEXT,

    -- Normalized path for comparisons. Example: lowercase on case-insensitive
    -- filesystems, canonical separators on Windows, resolved symlinks if desired.
    path_key          TEXT,

    -- File extension without the dot for file-like entries. Example: pdf, app.
    ext               TEXT,

    -- Extra searchable words from the platform. Examples: categories, app
    -- keywords, desktop-entry keywords, plugin tags.
    keywords          TEXT,

    -- User or app-provided alternate names. Examples: "vsc code" for VS Code.
    aliases           TEXT,

    -- How launch_payload should be interpreted by the executor.
    launch_kind       TEXT NOT NULL CHECK (
        launch_kind IN ('path', 'desktop_entry', 'app_id', 'url', 'command', 'action')
    ),

    -- Value passed to the launcher for execution. Examples: a path, desktop file
    -- id, Windows app id, URL, command string, or serialized action id.
    launch_payload    TEXT NOT NULL,

    -- Optional icon path or platform icon name.
    icon_path         TEXT,

    -- Flexible platform-specific metadata, usually JSON text. Examples: bundle id,
    -- AppUserModelID, desktop categories, localized names, executable path.
    platform_attrs    TEXT,

    -- File metadata for file-like entries. Leave defaults/nulls for non-files.
    size              INTEGER NOT NULL DEFAULT 0,
    mtime             INTEGER,
    ctime             INTEGER,

    -- Normalized flags used for filtering, ranking, and rescan decisions.
    is_hidden         INTEGER NOT NULL DEFAULT 0 CHECK (is_hidden IN (0, 1)),
    is_excluded       INTEGER NOT NULL DEFAULT 0 CHECK (is_excluded IN (0, 1)),
    is_pinned         INTEGER NOT NULL DEFAULT 0 CHECK (is_pinned IN (0, 1)),
    is_recent         INTEGER NOT NULL DEFAULT 0 CHECK (is_recent IN (0, 1)),
    is_binary         INTEGER NOT NULL DEFAULT 0 CHECK (is_binary IN (0, 1)),
    is_bundle_like    INTEGER NOT NULL DEFAULT 0 CHECK (is_bundle_like IN (0, 1)),
    content_indexed   INTEGER NOT NULL DEFAULT 0 CHECK (content_indexed IN (0, 1)),
    content_failed    INTEGER NOT NULL DEFAULT 0 CHECK (content_failed IN (0, 1)),
    needs_rescan      INTEGER NOT NULL DEFAULT 0 CHECK (needs_rescan IN (0, 1)),

    -- Unix timestamps controlled by the app, not SQLite wall-clock defaults.
    created_at        INTEGER NOT NULL,
    updated_at        INTEGER NOT NULL
);

-- Dynamic usage/ranking signals. These should survive rescans for the same
-- stable_key by keeping the search_entries row identity stable during upserts.
CREATE TABLE IF NOT EXISTS usage_stats (
    -- One usage row per searchable entry.
    entry_id          INTEGER PRIMARY KEY,

    -- Counts and timestamps are ranking signals. For example, a frequently
    -- launched app should rank above a weak text match.
    open_count        INTEGER NOT NULL DEFAULT 0,
    launch_count      INTEGER NOT NULL DEFAULT 0,
    selection_count   INTEGER NOT NULL DEFAULT 0,
    last_opened_at    INTEGER,
    last_launched_at  INTEGER,
    last_selected_at  INTEGER,
    boost             REAL NOT NULL DEFAULT 0,
    FOREIGN KEY (entry_id) REFERENCES search_entries(id) ON DELETE CASCADE
);

-- Tracks source scans so stale rows can be removed after platform-specific
-- app/file discovery completes.
CREATE TABLE IF NOT EXISTS source_scans (
    -- Matches search_entries.source.
    source            TEXT PRIMARY KEY,

    -- Unix timestamps for scan lifecycle/debugging.
    last_started_at   INTEGER,
    last_finished_at  INTEGER,
    last_error        TEXT,

    -- Increment before each full scan. New/updated rows receive this value.
    generation        INTEGER NOT NULL DEFAULT 0
);

-- Core lookup and filtering indexes.
CREATE INDEX IF NOT EXISTS idx_search_entries_source
    ON search_entries(source);
CREATE INDEX IF NOT EXISTS idx_search_entries_source_generation
    ON search_entries(source, source_generation);
CREATE INDEX IF NOT EXISTS idx_search_entries_kind
    ON search_entries(kind);
CREATE INDEX IF NOT EXISTS idx_search_entries_title
    ON search_entries(title);
CREATE INDEX IF NOT EXISTS idx_search_entries_path_key
    ON search_entries(path_key);
CREATE INDEX IF NOT EXISTS idx_search_entries_visible
    ON search_entries(is_excluded, is_hidden);
CREATE INDEX IF NOT EXISTS idx_search_entries_updated
    ON search_entries(updated_at);
CREATE INDEX IF NOT EXISTS idx_usage_last_selected
    ON usage_stats(last_selected_at);
CREATE INDEX IF NOT EXISTS idx_usage_last_launched
    ON usage_stats(last_launched_at);

-- FTS index for launcher-style candidate retrieval. Rust-side ranking can then
-- apply fuzzy scoring, prefix boosts, recency, pinning, and usage signals.
CREATE VIRTUAL TABLE IF NOT EXISTS search_entries_fts USING fts5(
    -- FTS columns mirror human-searchable text from search_entries.
    title,
    subtitle,
    path,
    keywords,
    aliases,
    content='search_entries',
    content_rowid='id',
    tokenize='unicode61',
    columnsize=0
);

CREATE TRIGGER IF NOT EXISTS search_entries_ai
AFTER INSERT ON search_entries BEGIN
    INSERT INTO search_entries_fts(rowid, title, subtitle, path, keywords, aliases)
    VALUES (new.id, new.title, new.subtitle, new.path, new.keywords, new.aliases);
END;

CREATE TRIGGER IF NOT EXISTS search_entries_ad
AFTER DELETE ON search_entries BEGIN
    INSERT INTO search_entries_fts(
        search_entries_fts,
        rowid,
        title,
        subtitle,
        path,
        keywords,
        aliases
    )
    VALUES (
        'delete',
        old.id,
        old.title,
        old.subtitle,
        old.path,
        old.keywords,
        old.aliases
    );
END;

CREATE TRIGGER IF NOT EXISTS search_entries_au
AFTER UPDATE OF title, subtitle, path, keywords, aliases ON search_entries BEGIN
    INSERT INTO search_entries_fts(
        search_entries_fts,
        rowid,
        title,
        subtitle,
        path,
        keywords,
        aliases
    )
    VALUES (
        'delete',
        old.id,
        old.title,
        old.subtitle,
        old.path,
        old.keywords,
        old.aliases
    );

    INSERT INTO search_entries_fts(rowid, title, subtitle, path, keywords, aliases)
    VALUES (new.id, new.title, new.subtitle, new.path, new.keywords, new.aliases);
END;

INSERT INTO search_entries_fts(search_entries_fts) VALUES ('rebuild');

COMMIT;

-- И да это я попросил нейронку все закомментировать