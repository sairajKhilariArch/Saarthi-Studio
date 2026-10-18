package com.saarthi.studio;

import android.database.Cursor;
import android.database.sqlite.SQLiteDatabase;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import org.junit.Test;
import org.junit.runner.RunWith;
import static org.junit.Assert.*;

@RunWith(AndroidJUnit4.class)
public class ConversationSearchInstrumentedTest {
    @Test public void actualSqlSearchAndOrganizationFiltersReturnExpectedRows() {
        SQLiteDatabase db = SQLiteDatabase.create(null);
        try {
            db.execSQL("CREATE TABLE conversations (_id INTEGER PRIMARY KEY,title TEXT,updated_at INTEGER,folder TEXT,tags TEXT,is_pinned INTEGER,is_favorite INTEGER)");
            db.execSQL("CREATE TABLE messages (_id INTEGER PRIMARY KEY,conversation_id INTEGER,content TEXT)");
            db.execSQL("INSERT INTO conversations VALUES (1,'Rust basics',1,'Rust','learning,android',1,0)");
            db.execSQL("INSERT INTO conversations VALUES (2,'Java interview',2,'College','android,qa',0,1)");
            db.execSQL("INSERT INTO conversations VALUES (3,'Python tips',3,'College','learning',0,0)");
            db.execSQL("INSERT INTO messages VALUES (11,1,'lifetimes and ownership')");
            db.execSQL("INSERT INTO messages VALUES (12,2,'classes and inheritance')");

            assertIds(db, ConversationSearch.fromTextAndFilter("lifetimes", "All conversations"), 20, 1L);
            assertIds(db, ConversationSearch.fromTextAndFilter("", "Folder: College"), 20, 2L, 3L);
            assertIds(db, ConversationSearch.fromTextAndFilter("", "Tag: android"), 20, 1L, 2L);
            assertIds(db, ConversationSearch.fromTextAndFilter("", "Pinned only"), 20, 1L);
            assertIds(db, ConversationSearch.fromTextAndFilter("", "Favorites only"), 20, 2L);
            assertIds(db, ConversationSearch.fromTextAndFilter("lifetimes", "Favorites only"), 20);
            assertIds(db, ConversationSearch.fromTextAndFilter("interview", "Tag: qa"), 20, 2L);
        } finally { db.close(); }
    }

    private static void assertIds(SQLiteDatabase db, ConversationSearch.Query query, int limit, long... expected) {
        long[] actual = new long[expected.length + 4];
        int count = 0;
        try (Cursor c = db.rawQuery(query.sql(), query.arguments(limit))) {
            while (c.moveToNext()) {
                if (count == actual.length) actual = java.util.Arrays.copyOf(actual, actual.length * 2);
                actual[count++] = c.getLong(0);
            }
        }
        actual = java.util.Arrays.copyOf(actual, count);
        assertArrayEquals(expected, actual);
    }
}
