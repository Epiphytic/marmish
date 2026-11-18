//! Shared ordered SQL catalog read; holds no writer authority.

use super::{CursorError, Result};
use rusqlite::Connection;

pub(crate) type Entry = (String, String, String, Option<String>);

pub(crate) fn inventory(conn: &Connection) -> Result<Vec<Entry>> {
    let temp: i64 = conn.query_row("SELECT count(*) FROM sqlite_temp_schema", [], |r| r.get(0))?;
    if temp != 0 {
        return Err(CursorError::Binding.into());
    }
    let mut query = conn.prepare(
        "SELECT type,name,tbl_name,sql FROM main.sqlite_schema ORDER BY type,name,tbl_name",
    )?;
    let rows = query.query_map([], |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)))?;
    Ok(rows.collect::<rusqlite::Result<_>>()?)
}
