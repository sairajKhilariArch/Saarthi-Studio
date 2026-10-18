package com.saarthi.studio;

import android.database.Cursor;
import android.database.sqlite.SQLiteDatabase;

/** Forward-only idempotent migrations, independently testable on Android SQLite. */
public final class DatabaseMigrations {
    private DatabaseMigrations() { }

    public static void migrateV4ToV5(SQLiteDatabase db) {
        addColumnIfMissing(db, "conversations", "folder", "TEXT NOT NULL DEFAULT ''");
        addColumnIfMissing(db, "conversations", "tags", "TEXT NOT NULL DEFAULT ''");
        addColumnIfMissing(db, "conversations", "is_pinned", "INTEGER NOT NULL DEFAULT 0");
        addColumnIfMissing(db, "conversations", "is_favorite", "INTEGER NOT NULL DEFAULT 0");
        db.execSQL("CREATE INDEX IF NOT EXISTS idx_conversations_organization ON conversations(is_pinned DESC, is_favorite DESC, updated_at DESC)");
    }

    public static void migrateV5ToV6(SQLiteDatabase db) {
        createImportedBackupsTable(db);
    }

    public static void createImportedBackupsTable(SQLiteDatabase db) {
        db.execSQL("CREATE TABLE IF NOT EXISTS imported_backups (sha256 TEXT PRIMARY KEY NOT NULL, imported_at INTEGER NOT NULL, conversation_count INTEGER NOT NULL DEFAULT 0)");
    }

    private static void addColumnIfMissing(SQLiteDatabase db, String table, String column, String definition) {
        try (Cursor cursor = db.rawQuery("PRAGMA table_info(" + table + ")", null)) {
            while (cursor.moveToNext()) if (column.equalsIgnoreCase(cursor.getString(1))) return;
        }
        db.execSQL("ALTER TABLE " + table + " ADD COLUMN " + column + " " + definition);
    }
}
