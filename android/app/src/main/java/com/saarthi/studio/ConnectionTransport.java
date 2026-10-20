package com.saarthi.studio;

/**
 * Small connection boundary shared by direct-router and desktop-host transports.
 * Streaming/UI orchestration remains in MainActivity for this step; later host-chat work
 * can implement the same boundary without copying the desktop Rust backend.
 */
interface ConnectionTransport {
    ConnectionMode mode();

    /** Return null when the supplied endpoint and credential are valid, otherwise a safe message. */
    String configurationError(String baseUrl, String credential);

    /** Resolve an operation path against this transport's API base. */
    String endpoint(String baseUrl, String operation);

    /** Return only this mode's credential. Router API keys and host tokens must not be mixed. */
    String credential(String directRouterApiKey, String desktopHostToken);

    /** True only when this transport implements OpenAI-compatible server-sent-event streaming. */
    boolean supportsStreaming();
}
