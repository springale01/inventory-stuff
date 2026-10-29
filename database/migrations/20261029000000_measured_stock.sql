-- Rebuild both sides of the composite foreign key while preserving existing data.
-- Counts remain INTEGER throughout; masses use finite REAL values in grams.
CREATE TABLE stocks_measured (
    item_id TEXT PRIMARY KEY NOT NULL REFERENCES items(id) ON DELETE RESTRICT,
    tracking_mode TEXT NOT NULL CHECK (tracking_mode IN ('bulk', 'individual')),
    quantity INTEGER,
    low_stock_at INTEGER DEFAULT 0,
    measurement TEXT NOT NULL DEFAULT 'count' CHECK (measurement IN ('count', 'mass')),
    quantity_grams REAL,
    low_stock_at_grams REAL,
    mass_per_piece REAL,
    CHECK (
        (measurement = 'count' AND low_stock_at IS NOT NULL AND low_stock_at >= 0
            AND quantity_grams IS NULL AND low_stock_at_grams IS NULL AND mass_per_piece IS NULL
            AND ((tracking_mode = 'bulk' AND quantity IS NOT NULL AND quantity >= 0)
                OR (tracking_mode = 'individual' AND quantity IS NULL)))
        OR (measurement = 'mass' AND tracking_mode = 'bulk'
            AND quantity IS NULL AND low_stock_at IS NULL
            AND quantity_grams IS NOT NULL AND quantity_grams BETWEEN 0 AND 1.7976931348623157e308
            AND low_stock_at_grams IS NOT NULL AND low_stock_at_grams BETWEEN 0 AND 1.7976931348623157e308
            AND (mass_per_piece IS NULL OR mass_per_piece > 0 AND mass_per_piece <= 1.7976931348623157e308))
    ),
    UNIQUE (item_id, tracking_mode)
) STRICT;

INSERT INTO stocks_measured (item_id, tracking_mode, quantity, low_stock_at)
    SELECT item_id, tracking_mode, quantity, low_stock_at FROM stocks;

CREATE TABLE stock_units_measured (
    id TEXT PRIMARY KEY NOT NULL,
    item_id TEXT NOT NULL,
    tracking_mode TEXT NOT NULL DEFAULT 'individual' CHECK (tracking_mode = 'individual'),
    serial_number TEXT CHECK (serial_number IS NULL OR length(trim(serial_number)) > 0),
    FOREIGN KEY (item_id, tracking_mode)
        REFERENCES stocks_measured(item_id, tracking_mode) ON DELETE RESTRICT,
    UNIQUE (item_id, serial_number)
) STRICT;
INSERT INTO stock_units_measured SELECT * FROM stock_units;
DROP TABLE stock_units;
DROP TABLE stocks;
ALTER TABLE stocks_measured RENAME TO stocks;
ALTER TABLE stock_units_measured RENAME TO stock_units;

-- History survives deletion of an empty catalog item; item_id keeps its identity.
CREATE TABLE stock_movements (
    id TEXT PRIMARY KEY NOT NULL,
    item_id TEXT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('added', 'removed')),
    measurement TEXT NOT NULL CHECK (measurement IN ('count', 'mass')),
    amount_count INTEGER,
    before_count INTEGER,
    after_count INTEGER,
    amount_grams REAL,
    before_grams REAL,
    after_grams REAL,
    amount_mass_per_piece REAL,
    before_mass_per_piece REAL,
    after_mass_per_piece REAL,
    reason TEXT NOT NULL CHECK (length(trim(reason)) > 0),
    occurred_at TEXT NOT NULL,
    CHECK (
        (measurement = 'count'
            AND amount_count IS NOT NULL AND amount_count > 0
            AND before_count IS NOT NULL AND before_count >= 0
            AND after_count IS NOT NULL AND after_count >= 0
            AND amount_grams IS NULL AND before_grams IS NULL AND after_grams IS NULL
            AND amount_mass_per_piece IS NULL AND before_mass_per_piece IS NULL AND after_mass_per_piece IS NULL)
        OR (measurement = 'mass'
            AND amount_count IS NULL AND before_count IS NULL AND after_count IS NULL
            AND amount_grams IS NOT NULL AND amount_grams > 0 AND amount_grams <= 1.7976931348623157e308
            AND before_grams IS NOT NULL AND before_grams BETWEEN 0 AND 1.7976931348623157e308
            AND after_grams IS NOT NULL AND after_grams BETWEEN 0 AND 1.7976931348623157e308)
    ),
    CHECK (amount_mass_per_piece IS NULL OR amount_mass_per_piece > 0 AND amount_mass_per_piece <= 1.7976931348623157e308),
    CHECK (before_mass_per_piece IS NULL OR before_mass_per_piece > 0 AND before_mass_per_piece <= 1.7976931348623157e308),
    CHECK (after_mass_per_piece IS NULL OR after_mass_per_piece > 0 AND after_mass_per_piece <= 1.7976931348623157e308)
) STRICT;
CREATE INDEX stock_movements_item_id ON stock_movements(item_id, id);
