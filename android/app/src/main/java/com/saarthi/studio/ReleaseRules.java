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
        return NetworkPolicy.shouldFallbackModel(status, message, false);
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
        return AttachmentPolicy.isSupportedTextExtension(extension);
    }

    static boolean isSupportedImageMimeType(String mime) {
        return AttachmentPolicy.isSupportedImageMimeType(mime);
    }

    static boolean isLikelyVisionModel(String model) {
        String value = model == null ? "" : model.toLowerCase(Locale.ROOT);
        return value.contains("vision") || value.contains("gemini") || value.contains("gpt-4o") ||
                value.contains("gpt-4.1") || value.contains("claude-3") || value.contains("claude-sonnet-4") ||
                value.contains("llava") || value.contains("qwen-vl") || value.contains("pixtral") || value.contains("minicpm-v");
    }
}
