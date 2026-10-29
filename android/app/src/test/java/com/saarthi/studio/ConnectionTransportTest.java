package com.saarthi.studio;

import org.junit.Test;

import static org.junit.Assert.*;

public class ConnectionTransportTest {
    @Test public void missingOrUnknownModeDefaultsToDirectRouterForExistingInstallations() {
        assertEquals(ConnectionMode.DIRECT_ROUTER, ConnectionMode.fromStoredValue(null));
        assertEquals(ConnectionMode.DIRECT_ROUTER, ConnectionMode.fromStoredValue(""));
        assertEquals(ConnectionMode.DIRECT_ROUTER, ConnectionMode.fromStoredValue("legacy-mode"));
        assertEquals(ConnectionMode.DIRECT_ROUTER, ConnectionMode.fromStoredValue("direct_router"));
        assertEquals(ConnectionMode.DESKTOP_HOST, ConnectionMode.fromStoredValue("desktop_host"));
    }

    @Test public void factorySelectsCorrectMode() {
        assertEquals(ConnectionMode.DIRECT_ROUTER, ConnectionTransportFactory.create("DIRECT_ROUTER").mode());
        assertEquals(ConnectionMode.DESKTOP_HOST, ConnectionTransportFactory.create("DESKTOP_HOST").mode());
    }

    @Test public void directRouterRequiresV1UrlAndApiKey() {
        ConnectionTransport direct = ConnectionTransportFactory.create(ConnectionMode.DIRECT_ROUTER);
        assertNull(direct.configurationError("http://192.168.1.10:3001/v1", "router-key"));
        assertNotNull(direct.configurationError("http://192.168.1.10:3001", "router-key"));
        assertNotNull(direct.configurationError("http://user:pass@example.test/v1", "router-key"));
        assertNotNull(direct.configurationError("https://example.test/v1?key=x", "router-key"));
        assertNotNull(direct.configurationError("https://example.test/v1", ""));
    }

    @Test public void directEndpointAndCredentialRemainUnchanged() {
        ConnectionTransport direct = new DirectRouterTransport();
        assertEquals("http://localhost:3001/v1/chat/completions",
                direct.endpoint("http://localhost:3001/v1/", "/chat/completions"));
        assertEquals("router-secret", direct.credential("router-secret", "host-secret"));
        assertTrue(direct.supportsStreaming());
    }

    @Test public void desktopHostRequiresHttpsAndAHostToken() {
        ConnectionTransport host = new DesktopHostTransport();
        assertNull(host.configurationError("https://saarthi.example.test", "host-token"));
        assertNull(host.configurationError("https://100.100.2.4:8766/", "host-token"));
        assertNotNull(host.configurationError("http://192.168.1.10:8766", "host-token"));
        assertNotNull(host.configurationError("https://example.test/api/host/v1", "host-token"));
        assertNotNull(host.configurationError("https://example.test?debug=1", "host-token"));
        assertNotNull(host.configurationError("https://example.test", ""));
    }

    @Test public void desktopHostMapsToVersionedApiAndNeverUsesRouterKey() {
        ConnectionTransport host = new DesktopHostTransport();
        assertEquals("https://desktop.example.test/api/host/v1/chat",
                host.endpoint("https://desktop.example.test/", "chat/completions"));
        assertEquals("https://desktop.example.test/api/host/v1/capabilities",
                host.endpoint("https://desktop.example.test", "capabilities"));
        assertEquals("host-secret", host.credential("router-secret", "host-secret"));
        assertTrue(host.supportsStreaming());
    }
}
