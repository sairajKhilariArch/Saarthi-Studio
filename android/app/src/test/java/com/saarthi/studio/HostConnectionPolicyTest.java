package com.saarthi.studio;

import org.junit.Test;

import static org.junit.Assert.*;

public class HostConnectionPolicyTest {
    @Test public void healthRequiresSuccessfulStatusAndHostIdentity() {
        assertTrue(HostConnectionPolicy.isHealthyResponse(200, true, "host-uuid", "v1"));
        assertFalse(HostConnectionPolicy.isHealthyResponse(401, true, "host-uuid", "v1"));
        assertFalse(HostConnectionPolicy.isHealthyResponse(200, false, "host-uuid", "v1"));
        assertFalse(HostConnectionPolicy.isHealthyResponse(200, true, "", "v1"));
        assertFalse(HostConnectionPolicy.isHealthyResponse(200, true, "host-uuid", ""));
    }

    @Test public void hostSettingsRequireHttpsAndAHostToken() {
        assertNotNull(HostConnectionPolicy.validationError("http://192.168.1.10:8766", "token"));
        assertNotNull(HostConnectionPolicy.validationError("https://192.168.1.10:8766", ""));
        assertNotNull(HostConnectionPolicy.validationError("https://host.example:8766/api/host/v1", "token"));
        assertNull(HostConnectionPolicy.validationError("https://host.example:8766", "token"));
    }

    @Test public void diagnosticsDoNotEchoLowLevelErrorDetails() {
        String secretLikeDetail = "SSL certificate validation failed for token=never-show-this";
        String message = HostConnectionPolicy.safeNetworkFailure(secretLikeDetail);
        assertTrue(message.contains("TLS certificate validation failed"));
        assertFalse(message.contains("never-show-this"));
        assertFalse(HostConnectionPolicy.safeHttpFailure(401).contains("response body"));
    }

    @Test public void statusFailuresHaveBoundedNonSensitiveMessages() {
        assertTrue(HostConnectionPolicy.safeHttpFailure(401).contains("rejected the token"));
        assertTrue(HostConnectionPolicy.safeHttpFailure(404).contains("not found"));
        assertTrue(HostConnectionPolicy.safeHttpFailure(503).contains("server error"));
    }
}
