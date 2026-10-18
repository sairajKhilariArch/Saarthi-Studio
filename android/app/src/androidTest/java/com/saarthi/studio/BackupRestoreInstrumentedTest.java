package com.saarthi.studio;

import android.content.Context;
import androidx.test.core.app.ApplicationProvider;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import org.json.JSONException;
import org.json.JSONObject;
import org.junit.Test;
import org.junit.runner.RunWith;
import java.nio.charset.StandardCharsets;
import java.util.Collections;
import static org.junit.Assert.*;

@RunWith(AndroidJUnit4.class)
public class BackupRestoreInstrumentedTest {
    private static final String TEST_DATABASE_NAME = "saarthi_history_test_backup_restore.sqlite3";
    @Test public void malformedOversizedUtf8AndImageMetadataAreRejected() throws Exception {
        try {
            BackupPolicy.parseObject("{ not-json".getBytes(StandardCharsets.UTF_8));
            fail("Malformed JSON should be rejected.");
        } catch (JSONException expected) { }

        try {
            BackupPolicy.parseObject(new byte[BackupPolicy.MAX_BACKUP_BYTES + 1]);
            fail("Oversized backup should be rejected before parsing.");
        } catch (IllegalArgumentException expected) { }

        try {
            BackupPolicy.parseObject(new byte[]{(byte) 0xC3, (byte) 0x28});
            fail("Malformed UTF-8 should be rejected.");
        } catch (JSONException expected) { }

        JSONObject parsed = BackupPolicy.parseObject("{\"format\":\"saarthi-studio-backup\",\"schema_version\":1}".getBytes(StandardCharsets.UTF_8));
        assertTrue(BackupPolicy.isSupportedBackup(parsed.optString("format"), parsed.optInt("schema_version", -1)));
        assertFalse(BackupPolicy.containsMalformedImageMarkers("text [[SAARTHI_IMAGE:image/png:iVBORw0KGgo=]]"));
        assertTrue(BackupPolicy.containsMalformedImageMarkers("[[SAARTHI_IMAGE:image/png:not-base64?!]]"));
        assertTrue(BackupPolicy.containsMalformedImageMarkers("broken [[SAARTHI_IMAGE:image/png:"));
        assertTrue(BackupPolicy.containsMalformedImageMarkers("[[SAARTHI_IMAGE:image/svg+xml:iVBORw0KGgo=]]"));
    }

    @Test public void exactBackupIsImportedOnceAndFailedImportDoesNotReserveFingerprint() {
        Context context = ApplicationProvider.getApplicationContext();
        context.deleteDatabase(TEST_DATABASE_NAME);
        ChatDatabase store = new ChatDatabase(context, TEST_DATABASE_NAME);
        try {
            String digest = BackupPolicy.sha256("test-backup-v1".getBytes(StandardCharsets.UTF_8));
            ChatDatabase.Message validMessage = new ChatDatabase.Message("user", "Preserve this message", "");
            ChatDatabase.ImportedConversation valid = new ChatDatabase.ImportedConversation("Imported test", Collections.singletonList(validMessage));
            ChatDatabase.PromptPreset preset = new ChatDatabase.PromptPreset(-1, "Imported test preset", "Keep outputs short", false);

            ChatDatabase.BackupImportResult first = store.importBackupIfNew(Collections.singletonList(valid), Collections.singletonList(preset), digest);
            assertFalse(first.duplicate);
            assertEquals(1, first.conversationIds.size());
            assertEquals(1, first.messageCount);
            assertEquals(1, first.presetCount);

            ChatDatabase.BackupImportResult second = store.importBackupIfNew(Collections.singletonList(valid), Collections.singletonList(preset), digest);
            assertTrue(second.duplicate);
            assertTrue(second.conversationIds.isEmpty());
            assertEquals("Preserve this message", store.allMessages(first.conversationIds.get(0), 10).get(0).content);
            assertEquals(1, store.customPromptPresets().size());

            String retryDigest = BackupPolicy.sha256("retryable-backup".getBytes(StandardCharsets.UTF_8));
            ChatDatabase.Message invalidMessage = new ChatDatabase.Message(null, "will fail", "");
            ChatDatabase.ImportedConversation invalid = new ChatDatabase.ImportedConversation("Should roll back", Collections.singletonList(invalidMessage));
            try {
                store.importBackupIfNew(Collections.singletonList(invalid), Collections.emptyList(), retryDigest);
                fail("Invalid NOT NULL message role must abort transaction.");
            } catch (RuntimeException expected) { }

            ChatDatabase.BackupImportResult recovered = store.importBackupIfNew(Collections.singletonList(valid), Collections.emptyList(), retryDigest);
            assertFalse("A failed import must not reserve the fingerprint", recovered.duplicate);
        } finally {
            store.close();
            context.deleteDatabase(TEST_DATABASE_NAME);
        }
    }
}
