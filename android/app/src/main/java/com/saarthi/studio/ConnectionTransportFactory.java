package com.saarthi.studio;

/** Creates a transport from the backward-compatible persisted connection mode. */
final class ConnectionTransportFactory {
    private static final ConnectionTransport DIRECT = new DirectRouterTransport();
    private static final ConnectionTransport HOST = new DesktopHostTransport();

    private ConnectionTransportFactory() { }

    static ConnectionTransport create(ConnectionMode mode) {
        return mode == ConnectionMode.DESKTOP_HOST ? HOST : DIRECT;
    }

    static ConnectionTransport create(String storedMode) {
        return create(ConnectionMode.fromStoredValue(storedMode));
    }
}
