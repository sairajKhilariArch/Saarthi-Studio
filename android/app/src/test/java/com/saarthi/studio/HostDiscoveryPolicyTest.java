package com.saarthi.studio;

import org.junit.Test;

import static org.junit.Assert.*;

public class HostDiscoveryPolicyTest {
    @Test public void capabilitiesRequireV1AndRequiredFeatures() {
        assertTrue(HostDiscoveryPolicy.capabilitiesCompatible(200, "1", true, true));
        assertFalse(HostDiscoveryPolicy.capabilitiesCompatible(401, "1", true, true));
        assertFalse(HostDiscoveryPolicy.capabilitiesCompatible(200, "2", true, true));
        assertFalse(HostDiscoveryPolicy.capabilitiesCompatible(200, "1", false, true));
        assertFalse(HostDiscoveryPolicy.capabilitiesCompatible(200, "1", true, false));
    }

    @Test public void modelListRequiresSuccessfulV1ResponseAndArray() {
        assertTrue(HostDiscoveryPolicy.modelListCompatible(200, "1", true, 0));
        assertTrue(HostDiscoveryPolicy.modelListCompatible(200, "1", true, 3));
        assertFalse(HostDiscoveryPolicy.modelListCompatible(500, "1", true, 3));
        assertFalse(HostDiscoveryPolicy.modelListCompatible(200, "2", true, 3));
        assertFalse(HostDiscoveryPolicy.modelListCompatible(200, "1", false, 3));
        assertFalse(HostDiscoveryPolicy.modelListCompatible(200, "1", true, -1));
    }

    @Test public void failuresAreSafeAndNeverIncludeResponseBodies() {
        assertTrue(HostDiscoveryPolicy.safeFailure("capabilities", 401).contains("rejected the token"));
        assertTrue(HostDiscoveryPolicy.safeFailure("model list", 404).contains("endpoint"));
        assertTrue(HostDiscoveryPolicy.safeFailure("model list", 503).contains("configuration"));
        assertFalse(HostDiscoveryPolicy.safeFailure("model list", 500).contains("router secret"));
    }
}
