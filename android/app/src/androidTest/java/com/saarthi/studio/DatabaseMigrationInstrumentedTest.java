package com.saarthi.studio;

import android.content.ContentValues;
import android.database.Cursor;
import android.database.sqlite.SQLiteDatabase;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import org.junit.Test;
import org.junit.runner.RunWith;
import static org.junit.Assert.*;

@RunWith(AndroidJUnit4.class)
public class DatabaseMigrationInstrumentedTest {
 @Test public void v4ToV6MigrationIsIdempotentAndPreservesChatIdsAndPresets() {
  SQLiteDatabase db=SQLiteDatabase.create(null);
  try {
   db.execSQL("CREATE TABLE conversations (_id INTEGER PRIMARY KEY AUTOINCREMENT,title TEXT NOT NULL,created_at INTEGER NOT NULL,updated_at INTEGER NOT NULL)");
   db.execSQL("CREATE TABLE messages (_id INTEGER PRIMARY KEY AUTOINCREMENT,conversation_id INTEGER NOT NULL,role TEXT NOT NULL,content TEXT NOT NULL,meta TEXT,created_at INTEGER NOT NULL)");
   db.execSQL("CREATE TABLE request_logs (_id INTEGER PRIMARY KEY AUTOINCREMENT,model TEXT NOT NULL,success INTEGER NOT NULL,status_code INTEGER NOT NULL DEFAULT 0,tokens INTEGER NOT NULL DEFAULT -1,duration_ms INTEGER NOT NULL DEFAULT -1,provider TEXT NOT NULL DEFAULT '',detail TEXT NOT NULL DEFAULT '',created_at INTEGER NOT NULL)");
   db.execSQL("CREATE TABLE prompt_presets (_id INTEGER PRIMARY KEY AUTOINCREMENT,name TEXT NOT NULL COLLATE NOCASE UNIQUE,content TEXT NOT NULL,built_in INTEGER NOT NULL DEFAULT 0,updated_at INTEGER NOT NULL)");
   db.execSQL("INSERT INTO conversations (_id,title,created_at,updated_at) VALUES (41,'Existing Rust chat',10,20)");
   db.execSQL("INSERT INTO messages (_id,conversation_id,role,content,meta,created_at) VALUES (71,41,'user','Explain borrowing','',11)");
   db.execSQL("INSERT INTO messages (_id,conversation_id,role,content,meta,created_at) VALUES (72,41,'assistant','Borrowing rules','ok',12)");
   db.execSQL("INSERT INTO prompt_presets (_id,name,content,built_in,updated_at) VALUES (91,'My custom prompt','Keep examples short',0,13)");
   db.beginTransaction();
   try { DatabaseMigrations.migrateV4ToV5(db); DatabaseMigrations.migrateV4ToV5(db); DatabaseMigrations.migrateV5ToV6(db); db.setTransactionSuccessful(); }
   finally { db.endTransaction(); }
   try(Cursor c=db.rawQuery("SELECT _id,title,folder,tags,is_pinned,is_favorite FROM conversations WHERE _id=41",null)) {
    assertTrue(c.moveToFirst()); assertEquals(41L,c.getLong(0)); assertEquals("Existing Rust chat",c.getString(1));
    assertEquals("",c.getString(2)); assertEquals("",c.getString(3)); assertEquals(0,c.getInt(4)); assertEquals(0,c.getInt(5));
   }
   try(Cursor c=db.rawQuery("SELECT _id,content FROM messages ORDER BY _id",null)) {
    assertTrue(c.moveToFirst()); assertEquals(71L,c.getLong(0)); assertEquals("Explain borrowing",c.getString(1));
    assertTrue(c.moveToNext()); assertEquals(72L,c.getLong(0)); assertEquals("Borrowing rules",c.getString(1)); assertFalse(c.moveToNext());
   }
   try(Cursor c=db.rawQuery("SELECT _id,name,content FROM prompt_presets WHERE _id=91",null)) {
    assertTrue(c.moveToFirst()); assertEquals(91L,c.getLong(0)); assertEquals("My custom prompt",c.getString(1)); assertEquals("Keep examples short",c.getString(2));
   }
   ContentValues record=new ContentValues(); record.put("sha256",repeat('a',64)); record.put("imported_at",99); record.put("conversation_count",1);
   assertTrue(db.insert("imported_backups",null,record)>0);
   assertEquals(-1L,db.insertWithOnConflict("imported_backups",null,record,SQLiteDatabase.CONFLICT_IGNORE));
   try(Cursor c=db.rawQuery("SELECT COUNT(*) FROM imported_backups",null)) { assertTrue(c.moveToFirst()); assertEquals(1,c.getInt(0)); }
  } finally { db.close(); }
 }
 private static String repeat(char value,int count) { char[] a=new char[count]; java.util.Arrays.fill(a,value); return new String(a); }
}
