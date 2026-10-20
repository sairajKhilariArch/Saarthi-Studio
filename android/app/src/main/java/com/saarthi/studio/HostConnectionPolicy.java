package com.saarthi.studio;

/** Pure helpers for validating and reporting an authenticated Desktop Host health check. */
final class HostConnectionPolicy {
    private HostConnectionPolicy() { }

    static String validationError(String baseUrl, String token) {
        return ConnectionTransportFactory.create(ConnectionMode.DESKTOP_HOST)
                .configurationError(baseUrl, token);
    }

    static boolean isHealthyResponse(int status, boolean ok, String hostId, String apiVersion) {
        return status >= 200 && status < 300
                && ok
                && hostId != null && !hostId.trim().isEmpty()
                && apiVersion != null && !apiVersion.trim().isEmpty();
    }

    /** Return a user-safe message that does not contain the upstream response body. */
    static String safeHttpFailure(int status) {
        if (status == 401 || status == 403) {
            return "Desktop Host rejected the token. Check that the saved host token is current.";
        }
        if (status == 404) {
            return "Desktop Host health endpoint was not found. Check the host API version and URL.";
        }
        if (status == 429) {
            return "Desktop Host is rate-limiting requests. Wait briefly and try again.";
        }
        if (status >= 500) {
            return "Desktop Host reported a server error. Check the desktop application's logs.";
        }
        if (status >= 200 && status < 300) {
            return "Desktop Host responded, but its health payload is invalid or incompatible.";
        }
        return "Desktop Host returned HTTP " + status + ". Check the URL, token, and host settings.";
    }

    /** Convert low-level failures to generic diagnostics; never display raw exception text. */
    static String safeNetworkFailure(String detail) {
        String value = detail == null ? "" : detail.toLowerCase(java.util.Locale.ROOT);
        if (value.contains("ssl") || value.contains("certificate") || value.contains("certpath")
                || value.contains("trust anchor") || value.contains("handshake")) {
            return "TLS certificate validation failed. Confirm that the host URL matches its certificate and that Android trusts its issuer.";
        }
        if (value.contains("timeout") || value.contains("timed out")) {
            return "Desktop Host connection timed out. Check the host address, listener, and firewall.";
        }
        if (value.contains("unable to resolve") || value.contains("unknownhost") || value.contains("resolve host")) {
            return "Desktop Host name could not be resolved. Check the host name and network connection.";
        }
        return "Could not connect to Desktop Host. Check its HTTPS URL, listener status, network, and firewall.";
    }
}
