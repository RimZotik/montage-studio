//! SQLite приложения (`studio.db` в папке настроек): указатель проектов и учет. Проект остается
//! самостоятельной папкой — база только помнит, где он и что с ним было. Схема растет миграциями
//! по `PRAGMA user_version` (`MIGRATIONS`, по одной на версию схемы).

use std::path::Path;
use std::sync::Mutex;

use rusqlite::Connection;

use crate::error::AppResult;

/// Миграции по порядку: индекс + 1 — версия схемы после нее.
const MIGRATIONS: &[&str] = &[
    // 1: недавние проекты, последние известные лимиты Claude.
    "create table projects (
        path text primary key,
        name text not null,
        width integer not null,
        height integer not null,
        fps integer not null,
        created_at integer not null,
        opened_at integer not null
    );
    create table claude_limits (
        id integer primary key check (id = 1),
        at integer not null,
        five_hour real,
        five_hour_resets integer,
        five_hour_status text,
        seven_day real,
        seven_day_resets integer,
        seven_day_status text
    );",
    // 2: расход Claude по ходам (замеры на проект).
    "create table claude_turns (
        id integer primary key,
        project text not null,
        session text not null,
        at integer not null,
        model text not null,
        input_tokens integer not null,
        output_tokens integer not null,
        cache_read integer not null,
        cache_create integer not null,
        cost_usd real not null,
        duration_ms integer not null
    );
    create index claude_turns_project on claude_turns (project, at);",
];

pub struct Db(Mutex<Connection>);

impl Db {
    pub fn open(path: &Path) -> AppResult<Self> {
        let conn = Connection::open(path)?;
        Self::init(conn)
    }

    #[cfg(test)]
    pub fn memory() -> AppResult<Self> {
        Self::init(Connection::open_in_memory()?)
    }

    fn init(mut conn: Connection) -> AppResult<Self> {
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "foreign_keys", "ON")?;
        migrate(&mut conn)?;
        Ok(Self(Mutex::new(conn)))
    }

    pub fn with<T>(&self, f: impl FnOnce(&Connection) -> AppResult<T>) -> AppResult<T> {
        let conn = self.0.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        f(&conn)
    }
}

fn migrate(conn: &mut Connection) -> AppResult<()> {
    let current: i64 = conn.pragma_query_value(None, "user_version", |row| row.get(0))?;
    for (index, sql) in MIGRATIONS.iter().enumerate().skip(usize::try_from(current).unwrap_or(0)) {
        let tx = conn.transaction()?;
        tx.execute_batch(sql)?;
        tx.pragma_update(None, "user_version", i64::try_from(index + 1).unwrap_or(i64::MAX))?;
        tx.commit()?;
        log::info!("database migrated to {}", index + 1);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn migrations_apply_once() {
        let db = Db::memory().unwrap();
        db.with(|c| {
            let v: i64 = c.pragma_query_value(None, "user_version", |r| r.get(0))?;
            assert_eq!(v, i64::try_from(MIGRATIONS.len()).unwrap());
            Ok(())
        })
        .unwrap();
        let mut conn = db.0.into_inner().unwrap();
        migrate(&mut conn).unwrap();
    }
}
