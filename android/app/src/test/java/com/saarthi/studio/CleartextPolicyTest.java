package com.saarthi.studio;

import static org.junit.Assert.assertEquals;
import static org.junit.Assert.assertFalse;
import static org.junit.Assert.assertNull;
import static org.junit.Assert.assertTrue;

import org.junit.Test;

public class CleartextPolicyTest {
    @Test public void loopbackAndLocalhostAreAllowed() {
        assertTrue(CleartextPolicy.isPrivateHost("localhost"));
        assertTrue(CleartextPolicy.isPrivateHost("app.localhost"));
        assertTrue(CleartextPolicy.isPrivateHost("127.0.0.1"));
        assertTrue(CleartextPolicy.isPrivateHost("127.9.8.7"));
        assertTrue(CleartextPolicy.isPrivateHost("[::1]"));
        assertNull(CleartextPolicy.insecureUrlError("http://127.0.0.1:3001/v1/models"));
        assertNull(CleartextPolicy.insecureUrlError("http://localhost:3001/v1"));
    }

    @Test public void privateLanMatchesEveryDocumentedRange() {
        assertTrue(CleartextPolicy.isPrivateHost("10.0.0.1"));
        assertTrue(CleartextPolicy.isPrivateHost("172.16.0.1"));
        assertTrue(CleartextPolicy.isPrivateHost("172.31.255.254"));
        assertTrue(CleartextPolicy.isPrivateHost("192.168.1.10"));
        assertTrue(CleartextPolicy.isPrivateHost("169.254.1.1"));
        assertTrue(CleartextPolicy.isPrivateHost("fd00::1"));
        assertTrue(CleartextPolicy.isPrivateHost("fe80::1"));
        assertNull(CleartextPolicy.insecureUrlError("http://192.168.1.10:3001/v1"));
    }

    @Test public void publicPlainHttpIsRefused() {
        assertFalse(CleartextPolicy.isPrivateHost("172.32.0.1"));
        assertFalse(CleartextPolicy.isPrivateHost("8.8.8.8"));
        assertFalse(CleartextPolicy.isPrivateHost("100.64.0.1"));
        assertFalse(CleartextPolicy.isPrivateHost("example.com"));
        String refusal = CleartextPolicy.insecureUrlError("http://example.com/v1");
        assertTrue(refusal != null && refusal.contains("HTTPS"));
        assertTrue(CleartextPolicy.insecureUrlError("http://8.8.8.8/v1").contains("HTTPS"));
    }

    @Test public void httpsAndLocalFilesAreLeftToThePlatform() {
        assertNull(CleartextPolicy.insecureUrlError("https://example.com/v1"));
        assertNull(CleartextPolicy.insecureUrlError("HTTPS://192.168.1.10/v1"));
        assertNull(CleartextPolicy.insecureUrlError(""));
        assertNull(CleartextPolicy.insecureUrlError("not a url"));
        assertEquals(true, CleartextPolicy.insecureUrlError("http://203.0.113.9/v1") != null);
    }

    @Test public void malformedOctetsAreNotMistakenForPrivateAddresses() {
        assertFalse(CleartextPolicy.isPrivateHost("10.0.0.999"));
        assertFalse(CleartextPolicy.isPrivateHost("10.0.0"));
        assertFalse(CleartextPolicy.isPrivateHost("10.0.0.1.example.com"));
        assertFalse(CleartextPolicy.isPrivateHost(""));
    }
}
