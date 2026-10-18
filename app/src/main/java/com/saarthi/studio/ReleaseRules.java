package com.saarthi.studio;

import java.util.ArrayList;
import java.util.LinkedHashSet;
import java.util.List;
import java.util.Locale;

/** Pure Java release rules used by the Android UI and unit tests. */
final class ReleaseRules {
    private ReleaseRules() { }

    static String normalizeModelList(String text) {
        LinkedHashSet<String> unique = new LinkedHashSet<>();
        if (text != null) {
            for (String item : text.split("[,\\n;]")) {
                String candidate = item.trim();
                if (!candidate.isEmpty() && candidate.length() <= 200) unique.add(candidate);
            }
        }
        return String.join(",", unique);
    }

    static List<String> modelCandidates(String primary, String fallbacks) {
        LinkedHashSet<String> models = new LinkedHashSet<>();
        String first = primary == null || primary.trim().isEmpty() ? "auto" : primary.trim();
        if (first.length() > 200) first = "auto";
        models.add(first);
        if (fallbacks != null) {
            for (String value : fallbacks.split("[,\\n;]")) {
                String candidate = value.trim();
                if (!candidate.isEmpty() && candidate.length() <= 200) models.add(candidate);
            }
        }
        return new ArrayList<>(models);
    }

    static boolean shouldFallbackModel(int status, String message) {
        String lower = message == null ? "" : message.toLowerCase(Locale.ROOT);
        if (status == 408 || status == 425 || status == 429 || status == 500 || status == 502 || status == 503 || status == 504) return true;
        return lower.contains("quota") || lower.contains("rate limit") || lower.contains("rate-limit") ||
                lower.contains("temporarily unavailable") || lower.contains("model not found") || lower.contains("unknown model") ||
                lower.contains("no available model") || lower.contains("provider unavailable") ||
                lower.contains("failed to connect") || lower.contains("connection reset") || lower.contains("timed out") || lower.contains("timeout");
    }

    static String sanitizeOrganizationText(String value, int maxLength) {
        String clean = value == null ? "" : value.replaceAll("[\\r\\n\\t]", " ").trim();
        int limit = Math.max(0, maxLength);
        return clean.length() > limit ? clean.substring(0, limit) : clean;
    }

    static String sanitizeTags(String value) {
        if (value == null || value.trim().isEmpty()) return "";
        LinkedHashSet<String> tags = new LinkedHashSet<>();
        for (String raw : value.split("[,;\\n]")) {
            String tag = sanitizeOrganizationText(raw, 32);
            if (!tag.isEmpty()) tags.add(tag);
            if (tags.size() >= 12) break;
        }
        return String.join(",", tags);
    }

    static boolean isSupportedTextExtension(String extension) {
        switch (extension == null ? "" : extension.toLowerCase(Locale.ROOT)) {
            case "rs": case "java": case "py": case "json": case "md": case "txt":
            case "c": case "h": case "cpp": case "hpp": case "js": case "jsx": case "ts": case "tsx":
            case "kt": case "kts": case "go": case "rb": case "php": case "swift": case "sql":
            case "sh": case "bash": case "log": case "csv": case "xml": case "html": case "css":
            case "yaml": case "yml": case "toml": case "ini": case "conf": case "gradle": case "properties":
                return true;
            default:
                return false;
        }
    }

    static boolean isSupportedImageMimeType(String mime) {
        if (mime == null) return false;
        switch (mime.toLowerCase(Locale.ROOT)) {
            case "image/jpeg": case "image/png": case "image/webp": case "image/gif":
                return true;
            default:
                return false;
        }
    }

    static boolean isLikelyVisionModel(String model) {
        String value = model == null ? "" : model.toLowerCase(Locale.ROOT);
        return value.contains("vision") || value.contains("gemini") || value.contains("gpt-4o") ||
                value.contains("gpt-4.1") || value.contains("claude-3") || value.contains("claude-sonnet-4") ||
                value.contains("llava") || value.contains("qwen-vl") || value.contains("pixtral") || value.contains("minicpm-v");
    }
}
