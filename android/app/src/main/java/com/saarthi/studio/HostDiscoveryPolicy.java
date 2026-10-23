package com.saarthi.studio;

/** Pure checks for the versioned Desktop Host capability and model-list responses. */
final class HostDiscoveryPolicy {
    private static final String SUPPORTED_API_VERSION = "1";

    private HostDiscoveryPolicy() { }

    static boolean capabilitiesCompatible(
            int status,
            String apiVersion,
            boolean nonStreamingChat,
            boolean modelList) {
        return isSuccess(status)
                && SUPPORTED_API_VERSION.equals(apiVersion)
                && nonStreamingChat
                && modelList;
    }

    static boolean modelListCompatible(int status, String apiVersion, boolean hasDataArray, int validCount) {
        return isSuccess(status)
                && SUPPORTED_API_VERSION.equals(apiVersion)
                && hasDataArray
                && validCount >= 0;
    }

    static String safeFailure(String stage, int status) {
        if (status == 401 || status == 403) {
            return "Desktop Host rejected the token. Check that the saved host token is current.";
        }
        if (status == 404) {
            return "Desktop Host is missing the required " + stage + " endpoint. Update the desktop app and check API compatibility.";
        }
        if (status == 429) {
            return "Desktop Host is rate-limiting requests. Wait briefly and try again.";
        }
        if (status >= 500) {
            return "Desktop Host could not provide " + stage + ". Check its configuration and logs.";
        }
        if (isSuccess(status)) {
            return "Desktop Host returned an incompatible or invalid " + stage + " response.";
        }
        return "Desktop Host returned HTTP " + status + " while checking " + stage + ".";
    }

    private static boolean isSuccess(int status) {
        return status >= 200 && status < 300;
    }
}
