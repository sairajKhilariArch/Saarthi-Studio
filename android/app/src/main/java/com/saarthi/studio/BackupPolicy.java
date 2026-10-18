package com.saarthi.studio;

import java.security.MessageDigest;
import java.security.NoSuchAlgorithmException;
import java.nio.ByteBuffer;
import java.nio.charset.CharacterCodingException;
import java.nio.charset.CodingErrorAction;
import java.nio.charset.StandardCharsets;
import org.json.JSONException;
import org.json.JSONObject;
import java.util.regex.Matcher;
import java.util.regex.Pattern;

/** Limits, schema checks and exact-file fingerprints for JSON backups. */
final class BackupPolicy {
    static final String FORMAT = "saarthi-studio-backup";
    static final int CURRENT_SCHEMA_VERSION = 3;
    static final int MAX_BACKUP_BYTES = 10 * 1024 * 1024;
    static final int MAX_CONVERSATIONS = 500;
    static final int MAX_MESSAGES_PER_CONVERSATION = 10000;
    static final int MAX_MESSAGES_TOTAL = 20000;
    static final int MAX_PROMPT_PRESETS = 100;
    static final int MAX_MESSAGE_CHARS = 1600000;
    static final int MAX_PRESET_NAME_CHARS = 80;
    static final int MAX_PRESET_CONTENT_CHARS = 8000;

    private static final Pattern STORED_IMAGE_MARKER = Pattern.compile("\\[\\[SAARTHI_IMAGE:([a-zA-Z0-9.+/-]+):([A-Za-z0-9+/=]+)\\]\\]");

    private BackupPolicy() { }

    static boolean isSupportedSchemaVersion(int version) {
        return version == 1 || version == 2 || version == CURRENT_SCHEMA_VERSION;
    }
    static boolean isSupportedBackup(String format, int version) {
        return FORMAT.equals(format) && isSupportedSchemaVersion(version);
    }
    static boolean isWithinSizeLimit(long sizeBytes) {
        return sizeBytes >= 0 && sizeBytes <= MAX_BACKUP_BYTES;
    }
    static boolean isValidMessageRole(String role) {
        return "user".equals(role) || "assistant".equals(role);
    }

    static boolean containsMalformedImageMarkers(String content) {
        if (content == null || !content.contains("[[SAARTHI_IMAGE:")) return false;
        Matcher matcher = STORED_IMAGE_MARKER.matcher(content);
        StringBuffer remaining = new StringBuffer();
        int imageCount = 0;
        long totalImageBytes = 0;
        while (matcher.find()) {
            int imageBytes = AttachmentPolicy.storedImageByteLength(matcher.group(1), matcher.group(2));
            if (imageBytes < 0) return true;
            imageCount++;
            totalImageBytes += imageBytes;
            if (imageCount > AttachmentPolicy.MAX_IMAGES || totalImageBytes > AttachmentPolicy.MAX_TOTAL_IMAGE_BYTES) return true;
            matcher.appendReplacement(remaining, "");
        }
        matcher.appendTail(remaining);
        return remaining.toString().contains("[[SAARTHI_IMAGE:");
    }

    /** Parses a bounded UTF-8 JSON object; rejects empty, oversized, malformed, or invalid UTF-8 input. */
    static JSONObject parseObject(byte[] bytes) throws JSONException {
        if (bytes == null || bytes.length == 0) throw new JSONException("Backup file is empty.");
        if (!isWithinSizeLimit(bytes.length)) throw new IllegalArgumentException("Backup file exceeds the 10 MiB limit.");
        final String json;
        try {
            json = StandardCharsets.UTF_8.newDecoder()
                    .onMalformedInput(CodingErrorAction.REPORT)
                    .onUnmappableCharacter(CodingErrorAction.REPORT)
                    .decode(ByteBuffer.wrap(bytes)).toString();
        } catch (CharacterCodingException e) {
            throw new JSONException("Backup file is not valid UTF-8.");
        }
        return new JSONObject(json);
    }
    static String sha256(byte[] content) {
        if (content == null) throw new IllegalArgumentException("Backup bytes are required.");
        try {
            byte[] digest = MessageDigest.getInstance("SHA-256").digest(content);
            StringBuilder out = new StringBuilder(digest.length * 2);
            for (byte value : digest) out.append(String.format(java.util.Locale.ROOT, "%02x", value & 0xff));
            return out.toString();
        } catch (NoSuchAlgorithmException impossible) {
            throw new IllegalStateException("SHA-256 is unavailable on this runtime.", impossible);
        }
    }
}
