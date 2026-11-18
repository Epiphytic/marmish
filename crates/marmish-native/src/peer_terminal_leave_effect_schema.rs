//! Exact local native Leave effect journal, available to the opt-in proof only.

use rusqlite::Connection;

use super::*;

const DDL: &str = "
CREATE TABLE peer_terminal_leave_effect_epoch (epoch INTEGER PRIMARY KEY CHECK (epoch=6)) STRICT;
INSERT INTO peer_terminal_leave_effect_epoch VALUES (6);
CREATE TRIGGER peer_terminal_leave_effect_epoch_insert BEFORE INSERT ON peer_terminal_leave_effect_epoch
BEGIN SELECT RAISE(ABORT,'immutable leave effect epoch'); END;
CREATE TRIGGER peer_terminal_leave_effect_epoch_update BEFORE UPDATE ON peer_terminal_leave_effect_epoch
BEGIN SELECT RAISE(ABORT,'immutable leave effect epoch'); END;
CREATE TRIGGER peer_terminal_leave_effect_epoch_delete BEFORE DELETE ON peer_terminal_leave_effect_epoch
BEGIN SELECT RAISE(ABORT,'immutable leave effect epoch'); END;

CREATE TABLE peer_terminal_leave_effect (
    class INTEGER NOT NULL CHECK (class IN (0,1)),
    generation INTEGER NOT NULL CHECK (generation>0),
    armed_at INTEGER NOT NULL CHECK (armed_at>0),
    image_digest BLOB NOT NULL CHECK (length(image_digest)=32),
    captured_at INTEGER CHECK (captured_at>=armed_at),
    event_id BLOB CHECK (length(event_id)=32),
    event_digest BLOB CHECK (length(event_digest)=32),
    signed_event BLOB CHECK (length(signed_event) BETWEEN 1 AND 65536),
    PRIMARY KEY (class,generation),
    FOREIGN KEY (class,generation) REFERENCES peer_terminal_native_leave(class,generation)
        ON UPDATE RESTRICT ON DELETE RESTRICT,
    CHECK (
        (captured_at IS NULL AND event_id IS NULL AND event_digest IS NULL AND signed_event IS NULL)
        OR (captured_at IS NOT NULL AND event_id IS NOT NULL AND event_digest IS NOT NULL
            AND signed_event IS NOT NULL)
    )
) STRICT;

CREATE TRIGGER peer_terminal_leave_effect_insert BEFORE INSERT ON peer_terminal_leave_effect
WHEN NEW.captured_at IS NOT NULL OR NOT EXISTS (
    SELECT 1 FROM peer_terminal_native_leave h JOIN peer_pending_terminal t
      ON t.class=h.class AND t.generation=h.generation
    JOIN peer_cursor_origin c ON c.origin=h.origin
    WHERE h.class=NEW.class AND h.generation=NEW.generation AND h.recovery_state=0
        AND t.step=0 AND t.decided_at<=NEW.armed_at AND NEW.armed_at<h.hold_deadline
        AND c.wall=NEW.armed_at)
BEGIN SELECT RAISE(ABORT,'exact leave effect reservation required'); END;

CREATE TRIGGER peer_terminal_leave_effect_update BEFORE UPDATE ON peer_terminal_leave_effect
WHEN OLD.class IS NOT NEW.class OR OLD.generation IS NOT NEW.generation
    OR OLD.armed_at IS NOT NEW.armed_at OR OLD.image_digest IS NOT NEW.image_digest
    OR OLD.captured_at IS NOT NULL OR NEW.captured_at IS NULL OR NOT EXISTS (
    SELECT 1 FROM peer_terminal_native_leave h JOIN peer_pending_terminal t
      ON t.class=h.class AND t.generation=h.generation
    JOIN peer_cursor_origin c ON c.origin=h.origin
    WHERE h.class=NEW.class AND h.generation=NEW.generation AND h.recovery_state=0
        AND t.step=0 AND NEW.captured_at<h.hold_deadline AND c.wall=NEW.captured_at)
BEGIN SELECT RAISE(ABORT,'exact leave effect capture required'); END;

CREATE TRIGGER peer_terminal_leave_effect_delete BEFORE DELETE ON peer_terminal_leave_effect
BEGIN SELECT RAISE(ABORT,'retain leave effect'); END;
";

pub(super) fn set_write(conn: &Connection, capture: bool, allow: bool) -> rusqlite::Result<()> {
    let operation = if capture { "UPDATE" } else { "INSERT" };
    if allow {
        conn.execute_batch(&format!(
            "DROP TRIGGER peer_terminal_leave_effect_deny_{operation}"
        ))
    } else {
        conn.execute_batch(&format!(
            "CREATE TRIGGER peer_terminal_leave_effect_deny_{operation}
             BEFORE {operation} ON peer_terminal_leave_effect
             BEGIN SELECT RAISE(ABORT,'leave effect owner required'); END"
        ))
    }
}

pub(super) fn install(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(DDL)?;
    set_write(conn, false, false)?;
    set_write(conn, true, false)
}

pub(super) fn verify_rows(conn: &Connection) -> std::result::Result<(), CursorError> {
    let valid: bool = conn.query_row(
        "SELECT (SELECT count(*) FROM peer_terminal_leave_effect_epoch WHERE epoch=6)=1
             AND NOT EXISTS (
                 SELECT 1 FROM peer_terminal_leave_effect e
                 LEFT JOIN peer_terminal_native_leave h
                   ON e.class=h.class AND e.generation=h.generation
                 LEFT JOIN peer_pending_terminal t
                   ON e.class=t.class AND e.generation=t.generation
                 WHERE h.class IS NULL OR t.class IS NULL OR t.step!=0
                     OR e.armed_at<t.decided_at OR e.armed_at>=h.hold_deadline
                     OR e.captured_at>=h.hold_deadline
             )",
        [],
        |row| row.get(0),
    )?;
    if valid {
        Ok(())
    } else {
        Err(CursorError::Binding)
    }
}
