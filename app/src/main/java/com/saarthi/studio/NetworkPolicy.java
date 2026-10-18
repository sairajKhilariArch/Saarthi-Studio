package com.saarthi.studio;

import java.util.Locale;

/** Pure network retry/fallback rules and diagnostic secret redaction. */
final class NetworkPolicy {
    private NetworkPolicy() { }

    static boolean shouldRetrySameModel(int status, String message, boolean partialOutput) {
        if (partialOutput) return false;
        if (status == 408 || status == 500 || status == 502 || status == 503 || status == 504) return true;
        String value = normalize(message);
        return value.contains("failed to connect") || value.contains("connection reset") ||
                value.contains("connection refused") || value.contains("timed out") ||
                value.contains("timeout") || value.contains("temporarily unavailable");
    }

    static boolean shouldFallbackModel(int status, String message, boolean partialOutput) {
        if (partialOutput) return false;
        if (status == 408 || status == 425 || status == 429 || status == 500 ||
                status == 502 || status == 503 || status == 504) return true;
        String value = normalize(message);
        return value.contains("quota") || value.contains("rate limit") || value.contains("rate-limit") ||
                value.contains("temporarily unavailable") || value.contains("model not found") ||
                value.contains("unknown model") || value.contains("no available model") ||
                value.contains("provider unavailable") || value.contains("failed to connect") ||
                value.contains("connection reset") || value.contains("timed out") || value.contains("timeout");
    }

    static String redactDiagnostic(String text, String secret) {
        String safe = text == null ? "Unknown error" : text.replaceAll("[\\r\\n\\t]+", " ").trim();
        if (secret != null && secret.length() >= 4) safe = safe.replace(secret, "[redacted]");
        safe = safe.replaceAll("(?i)(authorization\\s*[:=]\\s*bearer\\s+)[^\\s,;]+", "$1[redacted]");
        safe = safe.replaceAll("(?i)(api[-_ ]?key\\s*[:=]\\s*)[^\\s,;]+", "$1[redacted]");
        safe = safe.replaceAll("(?i)(access[-_ ]?token\\s*[:=]\\s*)[^\\s,;]+", "$1[redacted]");
        safe = safe.replaceAll("\\bsk-[A-Za-z0-9_-]{12,}\\b", "[redacted]");
        return safe.length() > 260 ? safe.substring(0, 257) + "…" : safe;
    }

    private static String normalize(String message) {
        return message == null ? "" : message.toLowerCase(Locale.ROOT);
    }
}
