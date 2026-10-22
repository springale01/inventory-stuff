-- UUIDs are stored as canonical UUID strings supplied by the application.
-- Enable foreign_keys on every connection (SQLx's SQLite options do by default).
-- SQLite INTEGER is signed: persist quantities/thresholds only through i64::MAX,
-- even though the domain currently uses u64.

CREATE TABLE items (
    id TEXT PRIMARY KEY NOT NULL,
    sku TEXT NOT NULL UNIQUE CHECK (length(trim(sku)) > 0),
    name TEXT NOT NULL CHECK (length(trim(name)) > 0),
    description TEXT
) STRICT;

-- One stock record per item. For individual tracking, quantity is derived by
-- counting stock_units; bulk stock stores its count directly here.
-- Create an item and its stock record together in an application transaction.
CREATE TABLE stocks (
    item_id TEXT PRIMARY KEY NOT NULL REFERENCES items(id) ON DELETE RESTRICT,
    tracking_mode TEXT NOT NULL CHECK (tracking_mode IN ('bulk', 'individual')),
    quantity INTEGER,
    low_stock_at INTEGER NOT NULL DEFAULT 0 CHECK (low_stock_at >= 0),
    CHECK (
        (tracking_mode = 'bulk' AND quantity IS NOT NULL AND quantity >= 0)
        OR (tracking_mode = 'individual' AND quantity IS NULL)
    ),
    -- Supports the composite foreign key that limits units to individual stock.
    UNIQUE (item_id, tracking_mode)
) STRICT;

CREATE TABLE stock_units (
    id TEXT PRIMARY KEY NOT NULL,
    item_id TEXT NOT NULL,
    tracking_mode TEXT NOT NULL DEFAULT 'individual'
        CHECK (tracking_mode = 'individual'),
    serial_number TEXT CHECK (
        serial_number IS NULL OR length(trim(serial_number)) > 0
    ),
    FOREIGN KEY (item_id, tracking_mode)
        REFERENCES stocks(item_id, tracking_mode) ON DELETE RESTRICT,
    -- Serial numbers are unique within an item; unnumbered units may repeat NULL.
    UNIQUE (item_id, serial_number)
) STRICT;

-- The UNIQUE index above also supports item_id lookups and per-item unit counts.
-- Remove individual unit rows before deleting their stock record. No movement
-- history is stored yet; Correction's audit payload is not defined in the domain.
