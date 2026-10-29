package com.saarthi.studio;

import java.net.URI;
import java.net.URISyntaxException;

/** Desktop Rust host transport. Streams the host's SSE chat frames token by token. */
final class DesktopHostTransport implements ConnectionTransport {
    private static final String API_PREFIX = "/api/host/v1/";

    @Override public ConnectionMode mode() { return ConnectionMode.DESKTOP_HOST; }

    @Override public String configurationError(String baseUrl, String credential) {
        URI uri = parseHttpsUri(baseUrl);
        if (uri == null || uri.getHost() == null || uri.getHost().trim().isEmpty()) {
            return "Desktop Host URL must be a valid HTTPS origin with no path, query, or fragment.";
        }
        String path = uri.getPath();
        if (path != null && !path.isEmpty() && !"/".equals(path)) {
            return "Desktop Host URL must be an origin only; do not append an API path.";
        }
        if (credential == null || credential.trim().isEmpty()) return "Enter the Desktop Host token.";
        return null;
    }

    @Override public String endpoint(String baseUrl, String operation) {
        String normalized = operation == null ? "" : operation.replaceAll("^/+", "");
        // Bridge existing OpenAI-like operation names to the versioned host API contract.
        if ("chat/completions".equals(normalized)) normalized = "chat";
        return DirectRouterTransport.join(baseUrl, API_PREFIX + normalized);
    }

    @Override public String credential(String directRouterApiKey, String desktopHostToken) {
        return desktopHostToken == null ? "" : desktopHostToken;
    }

    @Override public boolean supportsStreaming() { return true; }

    private static URI parseHttpsUri(String value) {
        try {
            URI uri = new URI(value == null ? "" : value.trim());
            if (!"https".equalsIgnoreCase(uri.getScheme())) return null;
            if (uri.getUserInfo() != null || uri.getQuery() != null || uri.getFragment() != null) return null;
            return uri;
        } catch (URISyntaxException ignored) {
            return null;
        }
    }
}
