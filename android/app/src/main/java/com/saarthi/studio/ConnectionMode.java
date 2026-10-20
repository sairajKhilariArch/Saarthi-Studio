package com.saarthi.studio;

import java.util.Locale;

/**
 * Persisted Android connection modes.
 * Missing/legacy values intentionally default to Direct Router so existing installs keep working.
 */
enum ConnectionMode {
    DIRECT_ROUTER,
    DESKTOP_HOST;

    static ConnectionMode fromStoredValue(String value) {
        if (value == null || value.trim().isEmpty()) return DIRECT_ROUTER;
        try {
            return ConnectionMode.valueOf(value.trim().toUpperCase(Locale.ROOT));
        } catch (IllegalArgumentException ignored) {
            // An unknown persisted mode must never silently route secrets to a different backend.
            return DIRECT_ROUTER;
        }
    }
}
