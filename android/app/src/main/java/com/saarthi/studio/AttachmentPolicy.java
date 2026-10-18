package com.saarthi.studio;

import java.util.Locale;

/** File-type and byte-budget policy shared by pickers and regression tests. */
final class AttachmentPolicy {
    static final int MAX_TEXT_FILE_BYTES = 512 * 1024;
    static final int MAX_TOTAL_TEXT_BYTES = 1024 * 1024;
    static final int MAX_TEXT_FILES = 3;
    static final int MAX_IMAGE_BYTES = 800 * 1024;
    static final int MAX_TOTAL_IMAGE_BYTES = 900 * 1024;
    static final int MAX_IMAGES = 2;

    private AttachmentPolicy() { }

    static boolean isSupportedTextExtension(String extension) {
        switch (extension == null ? "" : extension.toLowerCase(Locale.ROOT).replaceFirst("^\\.", "")) {
            case "rs": case "java": case "py": case "json": case "md": case "txt":
            case "c": case "h": case "cpp": case "hpp": case "js": case "jsx": case "ts": case "tsx":
            case "kt": case "kts": case "go": case "rb": case "php": case "swift": case "sql":
            case "sh": case "bash": case "log": case "csv": case "xml": case "html": case "css":
            case "yaml": case "yml": case "toml": case "ini": case "conf": case "gradle": case "properties":
                return true;
            default: return false;
        }
    }

    static boolean isSupportedImageMimeType(String mime) {
        if (mime == null) return false;
        switch (mime.toLowerCase(Locale.ROOT)) {
            case "image/jpeg": case "image/png": case "image/webp": case "image/gif": return true;
            default: return false;
        }
    }

    static boolean isValidStoredImageMarker(String mime, String encoded) {
        return storedImageByteLength(mime, encoded) > 0;
    }

    static int storedImageByteLength(String mime, String encoded) {
        if (!isSupportedImageMimeType(mime) || encoded == null || encoded.isEmpty()) return -1;
        try {
            byte[] decoded = java.util.Base64.getDecoder().decode(encoded);
            return decoded.length > 0 && decoded.length <= MAX_IMAGE_BYTES ? decoded.length : -1;
        } catch (IllegalArgumentException invalidBase64) {
            return -1;
        }
    }

    static boolean canAddTextFile(int count) { return count >= 0 && count < MAX_TEXT_FILES; }
    static boolean isTextFileWithinLimit(long bytes) { return bytes >= 0 && bytes <= MAX_TEXT_FILE_BYTES; }
    static boolean isTextTotalWithinLimit(long existing, long addition) {
        return existing >= 0 && addition >= 0 && existing + addition >= existing && existing + addition <= MAX_TOTAL_TEXT_BYTES;
    }
    static boolean canAddImage(int count) { return count >= 0 && count < MAX_IMAGES; }
    static boolean isImageWithinLimit(long bytes) { return bytes >= 0 && bytes <= MAX_IMAGE_BYTES; }
    static boolean isImageTotalWithinLimit(long existing, long addition) {
        return existing >= 0 && addition >= 0 && existing + addition >= existing && existing + addition <= MAX_TOTAL_IMAGE_BYTES;
    }
}
