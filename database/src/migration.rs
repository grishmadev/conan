use rusqlite::Connection;

/// Runs migration to add `is_friend` column to peer table if missing
pub fn run_is_friend_migration(conn: &Connection) -> Result<(), Box<dyn std::error::Error>> {
    let mut stmt = conn.prepare("PRAGMA table_info(peer);")?;
    let columns = stmt
        .query_map([], |r| r.get::<_, String>("name"))?
        .collect::<Result<Vec<_>, _>>()?;
    if columns.contains(&"is_friend".to_string()) {
        return Ok(());
    }
    conn.execute(
        "ALTER TABLE peer ADD COLUMN is_friend BOOLEAN DEFAULT FALSE",
        (),
    )?;

    conn.execute("UPDATE peer SET is_friend = TRUE;", ())?;
    Ok(())
}
