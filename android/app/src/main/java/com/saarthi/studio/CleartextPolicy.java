package com.saarthi.studio;

import java.net.URI;
import java.util.Locale;

/**
 * The app-level replacement for Android network-security-config CIDR rules.
 *
 * <p>The platform config cannot express private IP ranges, so it permits
 * cleartext and this policy is checked before every {@code HttpURLConnection}.
 * Plain HTTP is acceptable only on the machine itself or a private LAN; a
 * public host must be HTTPS. Keeping it pure makes the boundary unit-testable
 * without a device.
 */
final class CleartextPolicy {
    private CleartextPolicy() { }

    /** Null when the URL is allowed, otherwise a user-facing refusal. */
    static String insecureUrlError(String url) {
        if (url == null || url.trim().isEmpty()) return null;
        URI uri;
        try {
            uri = URI.create(url.trim());
        } catch (IllegalArgumentException malformed) {
            // Let URL report the malformed value with its own message.
            return null;
        }
        String scheme = uri.getScheme();
        if (scheme == null || !scheme.equalsIgnoreCase("http")) return null;
        String host = uri.getHost();
        if (host == null) return null;
        if (isPrivateHost(host)) return null;
        return "Plain HTTP is only allowed for localhost or a private LAN address. "
                + "Use HTTPS for " + host + ".";
    }

    static boolean isPrivateHost(String rawHost) {
        if (rawHost == null) return false;
        String host = rawHost.trim().toLowerCase(Locale.ROOT);
        if (host.startsWith("[") && host.endsWith("]")) {
            host = host.substring(1, host.length() - 1);
        }
        int zone = host.indexOf('%');
        if (zone >= 0) host = host.substring(0, zone);
        if (host.equals("localhost") || host.endsWith(".localhost")) return true;
        if (host.contains(":")) return isPrivateIpv6(host);

        String[] parts = host.split("\\.", -1);
        if (parts.length != 4) return false;
        int[] octets = new int[4];
        for (int i = 0; i < 4; i++) {
            if (parts[i].isEmpty() || parts[i].length() > 3) return false;
            try {
                octets[i] = Integer.parseInt(parts[i]);
            } catch (NumberFormatException error) {
                return false;
            }
            if (octets[i] < 0 || octets[i] > 255) return false;
        }
        int a = octets[0];
        int b = octets[1];
        // Loopback, RFC 1918, link-local (including cloud metadata) and the
        // unspecified address. Carrier-grade NAT (100.64/10) is intentionally
        // not treated as private: it is not a home LAN address.
        return a == 127 || a == 10 || (a == 172 && b >= 16 && b <= 31)
                || (a == 192 && b == 168) || (a == 169 && b == 254)
                || (a == 0 && b == 0);
    }

    private static boolean isPrivateIpv6(String host) {
        if (host.equals("::1") || host.equals("0:0:0:0:0:0:0:1")) return true;
        // IPv6 unique-local fc00::/7 and link-local fe80::/10.
        if (host.startsWith("fc") || host.startsWith("fd")) return true;
        return host.startsWith("fe8") || host.startsWith("fe9")
                || host.startsWith("fea") || host.startsWith("feb");
    }
}
