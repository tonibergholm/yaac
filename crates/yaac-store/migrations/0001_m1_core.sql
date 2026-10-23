-- M1 core schema: the tables from docs/architecture.md section 7 that M1 needs. Later milestones
-- add their own numbered files; this one must not change once it has shipped.
--
-- Foreign keys are DEFERRABLE INITIALLY DEFERRED, so they are checked at commit. A batch may insert
-- a child before its parent, and delete a subtree in any order.

CREATE TABLE meta (
    key   TEXT PRIMARY KEY,
    value TEXT NOT NULL
) STRICT;

CREATE TABLE profile (
    id         TEXT PRIMARY KEY,
    kind       TEXT NOT NULL CHECK (kind IN ('default', 'standard')),
    name       TEXT NOT NULL,
    cache_dir  TEXT NOT NULL,
    created_at INTEGER NOT NULL
) STRICT;

CREATE TABLE space (
    id            TEXT PRIMARY KEY,
    profile_id    TEXT NOT NULL REFERENCES profile (id) DEFERRABLE INITIALLY DEFERRED,
    name          TEXT NOT NULL,
    theme         TEXT,
    position      TEXT NOT NULL CHECK (length(position) > 0),
    agent_enabled INTEGER NOT NULL DEFAULT 0 CHECK (agent_enabled IN (0, 1))
) STRICT;

-- Target of the container's composite key below: a container's profile must equal its Space's.
CREATE UNIQUE INDEX space_id_profile ON space (id, profile_id);

CREATE TABLE container (
    id         TEXT PRIMARY KEY,
    profile_id TEXT NOT NULL REFERENCES profile (id) DEFERRABLE INITIALLY DEFERRED,
    space_id   TEXT,
    kind       TEXT NOT NULL CHECK (kind IN ('favorites', 'pinned', 'today', 'archive')),
    CHECK ((kind = 'favorites') = (space_id IS NULL)),
    FOREIGN KEY (space_id, profile_id) REFERENCES space (id, profile_id)
        DEFERRABLE INITIALLY DEFERRED
) STRICT;

CREATE TABLE node (
    id             TEXT PRIMARY KEY,
    container_id   TEXT NOT NULL REFERENCES container (id) DEFERRABLE INITIALLY DEFERRED,
    parent_id      TEXT,
    position       TEXT NOT NULL CHECK (length(position) > 0),
    kind           TEXT NOT NULL CHECK (kind IN ('tab', 'folder', 'split')),
    title          TEXT NOT NULL,
    url            TEXT,
    state          TEXT NOT NULL CHECK (state IN ('suspended')),
    is_unread      INTEGER NOT NULL DEFAULT 0 CHECK (is_unread IN (0, 1)),
    created_at     INTEGER NOT NULL,
    last_active_at INTEGER NOT NULL,
    updated_at     INTEGER NOT NULL,
    origin_device  TEXT,
    rev            INTEGER NOT NULL CHECK (rev >= 0),
    -- A child's container equals its parent's. A NULL parent_id skips the check (top level), and a
    -- missing parent fails it.
    FOREIGN KEY (parent_id, container_id) REFERENCES node (id, container_id)
        DEFERRABLE INITIALLY DEFERRED
) STRICT;

-- Target of the node's composite key above.
CREATE UNIQUE INDEX node_id_container ON node (id, container_id);

-- Sibling order: position, then id.
CREATE INDEX node_sibling_order ON node (container_id, parent_id, position, id);

CREATE TABLE session (
    window_id        TEXT PRIMARY KEY,
    space_id         TEXT NOT NULL REFERENCES space (id) DEFERRABLE INITIALLY DEFERRED,
    selected_node_id TEXT REFERENCES node (id) ON DELETE SET NULL DEFERRABLE INITIALLY DEFERRED,
    sidebar_width    INTEGER NOT NULL CHECK (sidebar_width >= 0)
) STRICT;
