package com.saarthi.studio;

import android.content.Context;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Locale;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/** SQLite persistence and data records for Saarthi Studio chat history. */
public final class ChatDatabase extends android.database.sqlite.SQLiteOpenHelper {
    public static final class PromptPreset {
        final long id; final String name, content; final boolean builtIn;
        PromptPreset(long id, String name, String content, boolean builtIn) {
            this.id = id; this.name = name == null ? "" : name; this.content = content == null ? "" : content; this.builtIn = builtIn;
        }
    }
    public static final class ImportedConversation {
        final String title, folder, tagsCsv;
        final boolean pinned, favorite;
        final List<Message> messages;
        ImportedConversation(String title, List<Message> messages) { this(title, messages, "", "", false, false); }
        ImportedConversation(String title, List<Message> messages, String folder, String tagsCsv, boolean pinned, boolean favorite) {
            this.title = title == null || title.trim().isEmpty() ? "Imported chat" : title.trim();
            this.messages = messages == null ? new ArrayList<>() : messages;
            this.folder = ReleaseRules.sanitizeOrganizationText(folder, 80);
            this.tagsCsv = ReleaseRules.sanitizeTags(tagsCsv);
            this.pinned = pinned; this.favorite = favorite;
        }
    }

    public static final class Message {
        final long id;
        final String role, content, meta;
        Message(String r, String c, String m) { this(-1L, r, c, m); }
        Message(long id, String r, String c, String m) {
            this.id = id; role = r; content = c == null ? "" : c; meta = m == null ? "" : m;
        }
    }

    public static final class Conversation {
        final long id;
        final String title, folder, tagsCsv;
        final long updatedAt;
        final boolean pinned, favorite;
        Conversation(long id, String title, long updatedAt) { this(id, title, updatedAt, "", "", false, false); }
        Conversation(long id, String title, long updatedAt, String folder, String tagsCsv, boolean pinned, boolean favorite) {
            this.id = id;
            this.title = title == null || title.trim().isEmpty() ? "New chat" : title;
            this.updatedAt = updatedAt;
            this.folder = ReleaseRules.sanitizeOrganizationText(folder, 80);
            this.tagsCsv = ReleaseRules.sanitizeTags(tagsCsv);
            this.pinned = pinned; this.favorite = favorite;
        }
    }

    public static final class UsageSummary {
        long totalRequests;
        long successfulRequests;
        long failedRequests;
        long reportedTokens;
        double averageDurationMs;
        long timedRequests;
    }

    public static final class RequestLog {
        final String model;
        final boolean success;
        final int statusCode;
        final int tokens;
        final long durationMs;
        final String provider;
        final String detail;
        final long createdAt;
        RequestLog(String model, boolean success, int statusCode, int tokens, long durationMs, String provider, String detail, long createdAt) {
            this.model = model == null ? "auto" : model;
            this.success = success;
            this.statusCode = statusCode;
            this.tokens = tokens;
            this.durationMs = durationMs;
            this.provider = provider == null ? "" : provider;
            this.detail = detail == null ? "" : detail;
            this.createdAt = createdAt;
        }
    }

    public static final class BackupImportResult {
        public final boolean duplicate;
        public final List<Long> conversationIds;
        public final int messageCount;
        public final int presetCount;
        BackupImportResult(boolean duplicate, List<Long> ids, int messages, int presets) {
            this.duplicate = duplicate;
            this.conversationIds = ids == null ? Collections.emptyList() : new ArrayList<>(ids);
            this.messageCount = messages;
            this.presetCount = presets;
        }
    }


    public ChatDatabase(Context c) { super(c, "saarthi_history.sqlite3", null, 6); }

        @Override public void onConfigure(android.database.sqlite.SQLiteDatabase db) {
            super.onConfigure(db);
            db.setForeignKeyConstraintsEnabled(true);
        }

        @Override public void onCreate(android.database.sqlite.SQLiteDatabase db) {
            db.execSQL("CREATE TABLE conversations (_id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL, folder TEXT NOT NULL DEFAULT '', tags TEXT NOT NULL DEFAULT '', is_pinned INTEGER NOT NULL DEFAULT 0, is_favorite INTEGER NOT NULL DEFAULT 0)");
            db.execSQL("CREATE TABLE messages (_id INTEGER PRIMARY KEY AUTOINCREMENT, conversation_id INTEGER NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL, meta TEXT, created_at INTEGER NOT NULL, FOREIGN KEY(conversation_id) REFERENCES conversations(_id) ON DELETE CASCADE)");
            db.execSQL("CREATE INDEX idx_messages_conversation_id ON messages(conversation_id, _id)");
            db.execSQL("CREATE INDEX IF NOT EXISTS idx_conversations_organization ON conversations(is_pinned DESC, is_favorite DESC, updated_at DESC)");
            createRequestLogTable(db);
            createPromptPresetsTable(db);
            seedDefaultPromptPresets(db);
            DatabaseMigrations.createImportedBackupsTable(db);
            long now = System.currentTimeMillis();
            android.content.ContentValues cv = new android.content.ContentValues();
            cv.put("title", "New chat"); cv.put("created_at", now); cv.put("updated_at", now);
            db.insert("conversations", null, cv);
        }

        @Override public void onUpgrade(android.database.sqlite.SQLiteDatabase db, int oldVersion, int newVersion) {
            if (oldVersion < 2) {
                long now = System.currentTimeMillis();
                db.execSQL("CREATE TABLE conversations (_id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL)");
                db.execSQL("INSERT INTO conversations (_id, title, created_at, updated_at) VALUES (1, 'Previous conversation', " + now + ", " + now + ")");
                // Version 1 stored every message in one flat history. Assign all old rows to that conversation.
                db.execSQL("ALTER TABLE messages ADD COLUMN conversation_id INTEGER NOT NULL DEFAULT 1");
                db.execSQL("CREATE INDEX idx_messages_conversation_id ON messages(conversation_id, _id)");
                db.execSQL("UPDATE conversations SET updated_at = COALESCE((SELECT MAX(created_at) FROM messages), created_at) WHERE _id = 1");
            }
            if (oldVersion < 3) {
                createRequestLogTable(db);
                // Backfill diagnostics from saved assistant replies so upgrading does not start with an empty dashboard.
                try (android.database.Cursor c = db.rawQuery("SELECT meta,content,created_at FROM messages WHERE role='assistant' ORDER BY _id", null)) {
                    while (c.moveToNext()) {
                        String meta = c.getString(0) == null ? "" : c.getString(0);
                        String content = c.getString(1) == null ? "" : c.getString(1);
                        boolean success = !meta.startsWith("Failed after") && !content.startsWith("FreeLLMAPI returned HTTP ");
                        int tokens = parseTokensFromMeta(meta);
                        long duration = parseDurationFromMeta(meta);
                        String provider = parseProviderFromMeta(meta);
                        int statusCode = parseStatusCode(content);
                        android.content.ContentValues values = new android.content.ContentValues();
                        values.put("model", "historical");
                        values.put("success", success ? 1 : 0);
                        values.put("status_code", statusCode);
                        values.put("tokens", tokens);
                        values.put("duration_ms", duration);
                        values.put("provider", provider);
                        values.put("detail", success ? "" : truncateDiagnostic(content));
                        values.put("created_at", c.getLong(2));
                        db.insert("request_logs", null, values);
                    }
                }
            }
            if (oldVersion < 4) {
                createPromptPresetsTable(db);
                seedDefaultPromptPresets(db);
            }
            if (oldVersion < 5) DatabaseMigrations.migrateV4ToV5(db);
            if (oldVersion < 6) DatabaseMigrations.migrateV5ToV6(db);
        }

        private static void createPromptPresetsTable(android.database.sqlite.SQLiteDatabase db) {
            db.execSQL("CREATE TABLE IF NOT EXISTS prompt_presets (_id INTEGER PRIMARY KEY AUTOINCREMENT, name TEXT NOT NULL COLLATE NOCASE UNIQUE, content TEXT NOT NULL, built_in INTEGER NOT NULL DEFAULT 0, updated_at INTEGER NOT NULL)");
        }

        private static void seedDefaultPromptPresets(android.database.sqlite.SQLiteDatabase db) {
            String[][] defaults = {
                    {"Explain Rust error", "Explain this Rust error step by step. Identify the root cause, show an idiomatic fix, and explain ownership, borrowing, or lifetimes when relevant. Keep the example minimal."},
                    {"Java QA test failure", "Analyze this Java QA automation test failure. Identify the likely root cause, propose a reliable fix, and suggest a focused regression test. Do not invent test output."},
                    {"Python debugging", "Debug this Python code. Explain the root cause, show the smallest safe correction, and mention any relevant edge cases."},
                    {"Code review", "Review the code for correctness, security, maintainability, performance, and testing gaps. Prioritize actionable findings with severity and file/line references when available."},
                    {"Debug with minimal reproduction", "Help debug this issue. First summarize what is known, list the most likely causes, then propose the smallest reproducible example and a verification step."},
                    {"Explain step by step", "Teach this concept from first principles. Use a short example, explain each important line, then give me one small practice exercise with its answer hidden until I ask."}
            };
            for (String[] item : defaults) {
                try (android.database.Cursor c = db.rawQuery("SELECT _id FROM prompt_presets WHERE name=?", new String[]{item[0]})) {
                    if (c.moveToFirst()) continue;
                }
                android.content.ContentValues values = new android.content.ContentValues();
                values.put("name", item[0]); values.put("content", item[1]); values.put("built_in", 1); values.put("updated_at", System.currentTimeMillis());
                db.insertWithOnConflict("prompt_presets", null, values, android.database.sqlite.SQLiteDatabase.CONFLICT_IGNORE);
            }
        }

        private static void createRequestLogTable(android.database.sqlite.SQLiteDatabase db) {
            db.execSQL("CREATE TABLE IF NOT EXISTS request_logs (_id INTEGER PRIMARY KEY AUTOINCREMENT, model TEXT NOT NULL, success INTEGER NOT NULL, status_code INTEGER NOT NULL DEFAULT 0, tokens INTEGER NOT NULL DEFAULT -1, duration_ms INTEGER NOT NULL DEFAULT -1, provider TEXT NOT NULL DEFAULT '', detail TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL)");
            db.execSQL("CREATE INDEX IF NOT EXISTS idx_request_logs_created_at ON request_logs(created_at DESC, _id DESC)");
        }

        private static int parseTokensFromMeta(String meta) {
            Matcher m = Pattern.compile("(\\d+)\\s+tokens").matcher(meta == null ? "" : meta);
            if (m.find()) { try { return Integer.parseInt(m.group(1)); } catch (Exception ignored) { } }
            return -1;
        }

        private static long parseDurationFromMeta(String meta) {
            Matcher m = Pattern.compile("([0-9]+(?:\\.[0-9]+)?)\\s*s").matcher(meta == null ? "" : meta);
            if (m.find()) { try { return (long)(Double.parseDouble(m.group(1)) * 1000.0); } catch (Exception ignored) { } }
            return -1;
        }

        private static String parseProviderFromMeta(String meta) {
            if (meta == null || meta.trim().isEmpty() || meta.startsWith("Failed after")) return "";
            String[] parts = meta.split(" · ", 3);
            return parts.length >= 3 ? parts[2] : "";
        }

        private static int parseStatusCode(String text) {
            Matcher m = Pattern.compile("HTTP\\s+(\\d{3})").matcher(text == null ? "" : text);
            if (m.find()) { try { return Integer.parseInt(m.group(1)); } catch (Exception ignored) { } }
            return 0;
        }

        private static String truncateDiagnostic(String text) {
            return NetworkPolicy.redactDiagnostic(text, null);
        }

        synchronized void addRequestLog(String model, boolean success, int statusCode, int tokens, long durationMs, String provider, String detail) {
            android.content.ContentValues values = new android.content.ContentValues();
            values.put("model", model == null || model.trim().isEmpty() ? "auto" : model.trim());
            values.put("success", success ? 1 : 0);
            values.put("status_code", statusCode);
            values.put("tokens", tokens);
            values.put("duration_ms", durationMs);
            values.put("provider", provider == null ? "" : provider);
            values.put("detail", success ? "" : NetworkPolicy.redactDiagnostic(detail, null));
            values.put("created_at", System.currentTimeMillis());
            getWritableDatabase().insert("request_logs", null, values);
        }

        synchronized List<String> recentModels(int limit) {
            List<String> result = new ArrayList<>();
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT model,MAX(created_at) AS newest FROM request_logs WHERE success=1 GROUP BY model ORDER BY newest DESC LIMIT ?",
                    new String[]{String.valueOf(Math.max(1, Math.min(limit, 20)))})) {
                while (c.moveToNext()) if (c.getString(0) != null && !c.getString(0).trim().isEmpty()) result.add(c.getString(0));
            }
            return result;
        }

        synchronized UsageSummary getUsageSummary() {
            UsageSummary summary = new UsageSummary();
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT COUNT(*), COALESCE(SUM(success),0), COALESCE(SUM(CASE WHEN success=0 THEN 1 ELSE 0 END),0), COALESCE(SUM(CASE WHEN tokens>=0 THEN tokens ELSE 0 END),0), COALESCE(AVG(CASE WHEN duration_ms>=0 THEN duration_ms END),0), COALESCE(SUM(CASE WHEN duration_ms>=0 THEN 1 ELSE 0 END),0) FROM request_logs", null)) {
                if (c.moveToFirst()) {
                    summary.totalRequests = c.getLong(0);
                    summary.successfulRequests = c.getLong(1);
                    summary.failedRequests = c.getLong(2);
                    summary.reportedTokens = c.getLong(3);
                    summary.averageDurationMs = c.getDouble(4);
                    summary.timedRequests = c.getLong(5);
                }
            }
            return summary;
        }

        synchronized List<RequestLog> recentRequestLogs(int limit) {
            List<RequestLog> result = new ArrayList<>();
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT model,success,status_code,tokens,duration_ms,provider,detail,created_at FROM request_logs ORDER BY created_at DESC, _id DESC LIMIT ?",
                    new String[]{String.valueOf(limit)})) {
                while (c.moveToNext()) result.add(new RequestLog(c.getString(0), c.getInt(1) != 0, c.getInt(2), c.getInt(3), c.getLong(4), c.getString(5), c.getString(6), c.getLong(7)));
            }
            return result;
        }

        synchronized long createConversation(String title) {
            android.database.sqlite.SQLiteDatabase db = getWritableDatabase();
            long now = System.currentTimeMillis();
            android.content.ContentValues values = new android.content.ContentValues();
            values.put("title", title == null || title.trim().isEmpty() ? "New chat" : title.trim());
            values.put("created_at", now); values.put("updated_at", now);
            return db.insert("conversations", null, values);
        }

        synchronized Conversation mostRecentConversation() {
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT _id,title,updated_at,folder,tags,is_pinned,is_favorite FROM conversations ORDER BY is_pinned DESC,is_favorite DESC,updated_at DESC,_id DESC LIMIT 1", null)) {
                if (c.moveToFirst()) return new Conversation(c.getLong(0), c.getString(1), c.getLong(2), c.getString(3), c.getString(4), c.getInt(5) != 0, c.getInt(6) != 0);
            }
            return null;
        }

        synchronized Conversation getConversation(long id) {
            if (id <= 0) return null;
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT _id,title,updated_at,folder,tags,is_pinned,is_favorite FROM conversations WHERE _id=?", new String[]{String.valueOf(id)})) {
                if (c.moveToFirst()) return new Conversation(c.getLong(0), c.getString(1), c.getLong(2), c.getString(3), c.getString(4), c.getInt(5) != 0, c.getInt(6) != 0);
            }
            return null;
        }

        synchronized List<Conversation> allConversations() {
            List<Conversation> result = new ArrayList<>();
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT _id,title,updated_at,folder,tags,is_pinned,is_favorite FROM conversations ORDER BY is_pinned DESC,is_favorite DESC,updated_at DESC,_id DESC", null)) {
                while (c.moveToNext()) result.add(new Conversation(c.getLong(0), c.getString(1), c.getLong(2), c.getString(3), c.getString(4), c.getInt(5) != 0, c.getInt(6) != 0));
            }
            return result;
        }

        synchronized List<Conversation> recentConversations(int limit) {
            int bounded = Math.max(1, Math.min(limit, 500));
            List<Conversation> result = new ArrayList<>();
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT _id,title,updated_at,folder,tags,is_pinned,is_favorite FROM conversations ORDER BY is_pinned DESC,is_favorite DESC,updated_at DESC,_id DESC LIMIT ?",
                    new String[]{String.valueOf(bounded)})) {
                while (c.moveToNext()) result.add(new Conversation(c.getLong(0), c.getString(1), c.getLong(2), c.getString(3), c.getString(4), c.getInt(5) != 0, c.getInt(6) != 0));
            }
            return result;
        }

        synchronized List<Conversation> searchConversations(String query) {
            return searchConversations(query, "All conversations", 250);
        }

        synchronized List<Conversation> searchConversations(String query, int limit) {
            return searchConversations(query, "All conversations", limit);
        }

        synchronized List<Conversation> searchConversations(String query, String filter, int limit) {
            List<Conversation> result = new ArrayList<>();
            ConversationSearch.Query search = ConversationSearch.fromTextAndFilter(query, filter);
            if (search.isUnfiltered()) return recentConversations(limit);
            try (android.database.Cursor c = getReadableDatabase().rawQuery(search.sql(), search.arguments(limit))) {
                while (c.moveToNext()) result.add(new Conversation(c.getLong(0), c.getString(1), c.getLong(2), c.getString(3), c.getString(4), c.getInt(5) != 0, c.getInt(6) != 0));
            }
            return result;
        }

        synchronized List<Long> importConversations(List<ImportedConversation> imported) {
            List<Long> ids = new ArrayList<>();
            android.database.sqlite.SQLiteDatabase db = getWritableDatabase();
            db.beginTransaction();
            try {
                for (ImportedConversation conversation : imported) {
                    long now = System.currentTimeMillis();
                    android.content.ContentValues conversationValues = new android.content.ContentValues();
                    conversationValues.put("title", conversation.title);
                    conversationValues.put("created_at", now);
                    conversationValues.put("updated_at", now);
                    conversationValues.put("folder", conversation.folder);
                    conversationValues.put("tags", conversation.tagsCsv);
                    conversationValues.put("is_pinned", conversation.pinned ? 1 : 0);
                    conversationValues.put("is_favorite", conversation.favorite ? 1 : 0);
                    long id = db.insert("conversations", null, conversationValues);
                    if (id < 0) throw new IllegalStateException("Could not create an imported conversation.");
                    ids.add(id);
                    for (Message message : conversation.messages) {
                        android.content.ContentValues messageValues = new android.content.ContentValues();
                        messageValues.put("conversation_id", id);
                        messageValues.put("role", message.role);
                        messageValues.put("content", message.content);
                        messageValues.put("meta", message.meta == null ? "" : message.meta);
                        messageValues.put("created_at", now);
                        if (db.insert("messages", null, messageValues) < 0) {
                            throw new IllegalStateException("Could not import a message.");
                        }
                    }
                }
                db.setTransactionSuccessful();
            } finally {
                db.endTransaction();
            }
            return ids;
        }

        synchronized BackupImportResult importBackupIfNew(List<ImportedConversation> imported, List<PromptPreset> presets, String sha256) {
            if (sha256 == null || !sha256.matches("[0-9a-f]{64}")) throw new IllegalArgumentException("A valid backup fingerprint is required.");
            android.database.sqlite.SQLiteDatabase db = getWritableDatabase();
            db.beginTransaction();
            try {
                try (android.database.Cursor c = db.rawQuery("SELECT 1 FROM imported_backups WHERE sha256=?", new String[]{sha256})) {
                    if (c.moveToFirst()) {
                        db.setTransactionSuccessful();
                        return new BackupImportResult(true, Collections.emptyList(), 0, 0);
                    }
                }
                if (imported == null || imported.isEmpty()) throw new IllegalArgumentException("No conversations were selected for import.");
                List<Long> ids = new ArrayList<>();
                int messageCount = 0;
                for (ImportedConversation conversation : imported) {
                    long now = System.currentTimeMillis();
                    android.content.ContentValues cv = new android.content.ContentValues();
                    cv.put("title", conversation.title); cv.put("created_at", now); cv.put("updated_at", now);
                    cv.put("folder", conversation.folder); cv.put("tags", conversation.tagsCsv);
                    cv.put("is_pinned", conversation.pinned ? 1 : 0); cv.put("is_favorite", conversation.favorite ? 1 : 0);
                    long id = db.insert("conversations", null, cv);
                    if (id < 0) throw new IllegalStateException("Could not create an imported conversation.");
                    ids.add(id);
                    for (Message message : conversation.messages) {
                        android.content.ContentValues mv = new android.content.ContentValues();
                        mv.put("conversation_id", id); mv.put("role", message.role); mv.put("content", message.content);
                        mv.put("meta", message.meta == null ? "" : message.meta); mv.put("created_at", now);
                        if (db.insert("messages", null, mv) < 0) throw new IllegalStateException("Could not import a message.");
                        messageCount++;
                    }
                }
                int presetCount = 0;
                if (presets != null) for (PromptPreset preset : presets) {
                    String base = preset.name == null || preset.name.trim().isEmpty() ? "Imported preset" : preset.name.trim();
                    String candidate = base.length() > 80 ? base.substring(0, 80) : base;
                    int suffix = 2;
                    while (promptPresetNameExists(db, candidate)) {
                        String ending = " (Imported " + suffix++ + ")";
                        int keep = Math.max(1, 80 - ending.length());
                        candidate = base.substring(0, Math.min(base.length(), keep)) + ending;
                    }
                    android.content.ContentValues pv = new android.content.ContentValues();
                    pv.put("name", candidate); pv.put("content", preset.content); pv.put("built_in", 0); pv.put("updated_at", System.currentTimeMillis());
                    if (db.insert("prompt_presets", null, pv) >= 0) presetCount++;
                }
                android.content.ContentValues fp = new android.content.ContentValues();
                fp.put("sha256", sha256); fp.put("imported_at", System.currentTimeMillis()); fp.put("conversation_count", ids.size());
                if (db.insert("imported_backups", null, fp) < 0) throw new IllegalStateException("Could not record the backup import.");
                db.setTransactionSuccessful();
                return new BackupImportResult(false, ids, messageCount, presetCount);
            } finally { db.endTransaction(); }
        }

        synchronized void renameConversation(long id, String title) {
            android.content.ContentValues values = new android.content.ContentValues();
            values.put("title", title.trim()); values.put("updated_at", System.currentTimeMillis());
            getWritableDatabase().update("conversations", values, "_id=?", new String[]{String.valueOf(id)});
        }

        synchronized void saveConversationOrganization(long id, String folder, String tags, boolean pinned, boolean favorite) {
            android.content.ContentValues values = new android.content.ContentValues();
            values.put("folder", ReleaseRules.sanitizeOrganizationText(folder, 80));
            values.put("tags", ReleaseRules.sanitizeTags(tags));
            values.put("is_pinned", pinned ? 1 : 0);
            values.put("is_favorite", favorite ? 1 : 0);
            values.put("updated_at", System.currentTimeMillis());
            getWritableDatabase().update("conversations", values, "_id=?", new String[]{String.valueOf(id)});
        }

        synchronized void renameIfDefaultTitle(long id, String title) {
            Conversation c = getConversation(id);
            if (c != null && "New chat".equals(c.title)) renameConversation(id, title);
        }

        synchronized void deleteConversation(long id) {
            android.database.sqlite.SQLiteDatabase db = getWritableDatabase();
            db.beginTransaction();
            try {
                // Explicit delete keeps deletion correct for rows migrated from the old schema too.
                db.delete("messages", "conversation_id=?", new String[]{String.valueOf(id)});
                db.delete("conversations", "_id=?", new String[]{String.valueOf(id)});
                db.setTransactionSuccessful();
            } finally { db.endTransaction(); }
        }

        synchronized void addMessage(long conversationId, String role, String content, String meta) {
            android.database.sqlite.SQLiteDatabase db = getWritableDatabase();
            long now = System.currentTimeMillis();
            android.content.ContentValues values = new android.content.ContentValues();
            values.put("conversation_id", conversationId); values.put("role", role); values.put("content", content);
            values.put("meta", meta); values.put("created_at", now);
            db.insert("messages", null, values);
            android.content.ContentValues updated = new android.content.ContentValues(); updated.put("updated_at", now);
            db.update("conversations", updated, "_id=?", new String[]{String.valueOf(conversationId)});
        }

        synchronized Message getMessage(long conversationId, long messageId) {
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT _id,role,content,meta FROM messages WHERE conversation_id=? AND _id=?",
                    new String[]{String.valueOf(conversationId), String.valueOf(messageId)})) {
                if (c.moveToFirst()) return new Message(c.getLong(0), c.getString(1), c.getString(2), c.getString(3));
            }
            return null;
        }

        synchronized Message latestUserMessage(long conversationId) {
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT _id,role,content,meta FROM messages WHERE conversation_id=? AND role='user' ORDER BY _id DESC LIMIT 1",
                    new String[]{String.valueOf(conversationId)})) {
                if (c.moveToFirst()) return new Message(c.getLong(0), c.getString(1), c.getString(2), c.getString(3));
            }
            return null;
        }

        synchronized Message lastUserMessageBefore(long conversationId, long messageId) {
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT _id,role,content,meta FROM messages WHERE conversation_id=? AND role='user' AND _id<? ORDER BY _id DESC LIMIT 1",
                    new String[]{String.valueOf(conversationId), String.valueOf(messageId)})) {
                if (c.moveToFirst()) return new Message(c.getLong(0), c.getString(1), c.getString(2), c.getString(3));
            }
            return null;
        }

        synchronized boolean isLatestMessage(long conversationId, long messageId) {
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT _id FROM messages WHERE conversation_id=? ORDER BY _id DESC LIMIT 1",
                    new String[]{String.valueOf(conversationId)})) {
                return c.moveToFirst() && c.getLong(0) == messageId;
            }
        }

        synchronized void deleteMessage(long conversationId, long messageId) {
            getWritableDatabase().delete("messages", "conversation_id=? AND _id=?", new String[]{String.valueOf(conversationId), String.valueOf(messageId)});
            android.content.ContentValues updated = new android.content.ContentValues(); updated.put("updated_at", System.currentTimeMillis());
            getWritableDatabase().update("conversations", updated, "_id=?", new String[]{String.valueOf(conversationId)});
        }

        synchronized void editUserMessageAndTrim(long conversationId, long messageId, String content) {
            android.database.sqlite.SQLiteDatabase db = getWritableDatabase();
            db.beginTransaction();
            try {
                android.content.ContentValues values = new android.content.ContentValues(); values.put("content", content); values.put("meta", "");
                int changed = db.update("messages", values, "conversation_id=? AND _id=? AND role='user'",
                        new String[]{String.valueOf(conversationId), String.valueOf(messageId)});
                if (changed != 1) throw new IllegalStateException("That user message no longer exists.");
                db.delete("messages", "conversation_id=? AND _id>?", new String[]{String.valueOf(conversationId), String.valueOf(messageId)});
                android.content.ContentValues updated = new android.content.ContentValues(); updated.put("updated_at", System.currentTimeMillis());
                db.update("conversations", updated, "_id=?", new String[]{String.valueOf(conversationId)});
                db.setTransactionSuccessful();
            } finally { db.endTransaction(); }
        }

        synchronized List<PromptPreset> allPromptPresets() {
            List<PromptPreset> values = new ArrayList<>();
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT _id,name,content,built_in FROM prompt_presets ORDER BY built_in DESC, name COLLATE NOCASE", null)) {
                while (c.moveToNext()) values.add(new PromptPreset(c.getLong(0), c.getString(1), c.getString(2), c.getInt(3) != 0));
            }
            return values;
        }

        synchronized List<PromptPreset> customPromptPresets() {
            List<PromptPreset> values = new ArrayList<>();
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT _id,name,content,built_in FROM prompt_presets WHERE built_in=0 ORDER BY name COLLATE NOCASE", null)) {
                while (c.moveToNext()) values.add(new PromptPreset(c.getLong(0), c.getString(1), c.getString(2), false));
            }
            return values;
        }

        synchronized void savePromptPreset(long id, String name, String content, boolean builtIn) {
            android.content.ContentValues values = new android.content.ContentValues();
            values.put("name", name.trim()); values.put("content", content.trim());
            values.put("built_in", builtIn ? 1 : 0); values.put("updated_at", System.currentTimeMillis());
            if (id > 0) {
                int updated = getWritableDatabase().update("prompt_presets", values, "_id=?", new String[]{String.valueOf(id)});
                if (updated != 1) throw new IllegalStateException("The selected prompt preset no longer exists.");
            } else {
                if (getWritableDatabase().insert("prompt_presets", null, values) < 0) throw new IllegalStateException("A preset with that name already exists.");
            }
        }

        synchronized void deletePromptPreset(long id) {
            getWritableDatabase().delete("prompt_presets", "_id=? AND built_in=0", new String[]{String.valueOf(id)});
        }

        synchronized int importPromptPresets(List<PromptPreset> presets) {
            if (presets == null || presets.isEmpty()) return 0;
            android.database.sqlite.SQLiteDatabase db = getWritableDatabase();
            int imported = 0;
            db.beginTransaction();
            try {
                for (PromptPreset preset : presets) {
                    String base = preset.name == null ? "Imported preset" : preset.name.trim();
                    if (base.isEmpty()) base = "Imported preset";
                    String candidate = base.length() > 80 ? base.substring(0, 80) : base;
                    int suffix = 2;
                    while (promptPresetNameExists(db, candidate)) {
                        String ending = " (Imported " + suffix++ + ")";
                        int keep = Math.max(1, 80 - ending.length());
                        candidate = base.substring(0, Math.min(base.length(), keep)) + ending;
                    }
                    android.content.ContentValues values = new android.content.ContentValues();
                    values.put("name", candidate); values.put("content", preset.content); values.put("built_in", 0); values.put("updated_at", System.currentTimeMillis());
                    if (db.insert("prompt_presets", null, values) >= 0) imported++;
                }
                db.setTransactionSuccessful();
            } finally { db.endTransaction(); }
            return imported;
        }

        private static boolean promptPresetNameExists(android.database.sqlite.SQLiteDatabase db, String name) {
            try (android.database.Cursor c = db.rawQuery("SELECT 1 FROM prompt_presets WHERE name=?", new String[]{name})) { return c.moveToFirst(); }
        }

        synchronized String matchingPreview(long conversationId, String query) {
            String term = query == null ? "" : query.trim();
            if (term.isEmpty()) return "";
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT content FROM messages WHERE conversation_id=? AND instr(lower(content), lower(?)) > 0 ORDER BY _id DESC LIMIT 1",
                    new String[]{String.valueOf(conversationId), term})) {
                if (c.moveToFirst()) return ConversationSearch.matchingPreview(c.getString(0), term);
            }
            return "";
        }

        synchronized String preview(long conversationId) {
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT content FROM messages WHERE conversation_id=? ORDER BY _id DESC LIMIT 1",
                    new String[]{String.valueOf(conversationId)})) {
                if (c.moveToFirst()) {
                    String value = c.getString(0).replaceAll("\\s+", " ").trim();
                    return value.length() > 90 ? value.substring(0, 87) + "…" : value;
                }
            }
            return "";
        }

        synchronized List<Message> latestMessages(long conversationId, int limit) {
            List<Message> result = new ArrayList<>();
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT _id,role,content,meta FROM messages WHERE conversation_id=? ORDER BY _id DESC LIMIT ?",
                    new String[]{String.valueOf(conversationId), String.valueOf(limit)})) {
                while (c.moveToNext()) result.add(new Message(c.getLong(0), c.getString(1), c.getString(2), c.getString(3)));
            }
            Collections.reverse(result); return result;
        }

        synchronized int countMessages(long conversationId) {
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT COUNT(*) FROM messages WHERE conversation_id=?",
                    new String[]{String.valueOf(conversationId)})) {
                return c.moveToFirst() ? c.getInt(0) : 0;
            }
        }

        synchronized List<Message> allMessages(long conversationId, int limit) {
            List<Message> result = new ArrayList<>();
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT _id,role,content,meta FROM messages WHERE conversation_id=? ORDER BY _id DESC LIMIT ?",
                    new String[]{String.valueOf(conversationId), String.valueOf(limit)})) {
                while (c.moveToNext()) result.add(new Message(c.getLong(0), c.getString(1), c.getString(2), c.getString(3)));
            }
            Collections.reverse(result); return result;
        }
    }
