package com.saarthi.studio;

import java.net.URI;
import java.net.URISyntaxException;

/** Existing FreeLLMAPI/OpenAI-compatible direct mode. */
final class DirectRouterTransport implements ConnectionTransport {
    @Override public ConnectionMode mode() { return ConnectionMode.DIRECT_ROUTER; }

    @Override public String configurationError(String baseUrl, String credential) {
        URI uri = parseHttpUri(baseUrl);
        if (uri == null || uri.getHost() == null || uri.getHost().trim().isEmpty()) {
            return "Enter a valid Direct Router URL.";
        }
        String path = uri.getPath() == null ? "" : uri.getPath().replaceAll("/+$", "");
        if (!"/v1".equals(path)) return "Direct Router URL must end in /v1.";
        if (credential == null || credential.trim().isEmpty()) return "Enter the unified Direct Router API key.";
        return null;
    }

    @Override public String endpoint(String baseUrl, String operation) {
        return join(baseUrl, operation);
    }

    @Override public String credential(String directRouterApiKey, String desktopHostToken) {
        return directRouterApiKey == null ? "" : directRouterApiKey;
    }

    @Override public boolean supportsStreaming() { return true; }

    private static URI parseHttpUri(String value) {
        try {
            URI uri = new URI(value == null ? "" : value.trim());
            String scheme = uri.getScheme();
            if (scheme == null || !(scheme.equalsIgnoreCase("http") || scheme.equalsIgnoreCase("https"))) return null;
            if (uri.getUserInfo() != null || uri.getQuery() != null || uri.getFragment() != null) return null;
            return uri;
        } catch (URISyntaxException ignored) {
            return null;
        }
    }

    static String join(String baseUrl, String operation) {
        String base = baseUrl == null ? "" : baseUrl.trim().replaceAll("/+$", "");
        String path = operation == null ? "" : operation.replaceAll("^/+", "");
        return base + "/" + path;
    }
}
