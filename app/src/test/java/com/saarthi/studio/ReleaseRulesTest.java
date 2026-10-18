package com.saarthi.studio;

import org.junit.Test;
import java.util.Arrays;
import static org.junit.Assert.*;

public class ReleaseRulesTest {
    @Test public void modelCandidatesKeepPrimaryAndOrderedUniqueFallbacks() {
        assertEquals(Arrays.asList("primary", "backup-a", "backup-b"),
                ReleaseRules.modelCandidates(" primary ", "backup-a, backup-b;primary\nbackup-a"));
    }

    @Test public void modelCandidatesDefaultToAutoAndIgnoreOversizedFallbacks() {
        assertEquals(Arrays.asList("auto", "safe-model"),
                ReleaseRules.modelCandidates("  ", "safe-model," + repeat('x', 201)));
    }

    @Test public void normalizedModelListTrimsAndDeduplicates() {
        assertEquals("alpha,beta", ReleaseRules.normalizeModelList(" alpha ; beta,alpha\nbeta "));
    }

    @Test public void fallbackPolicyOnlyMatchesTransientOrRecognizedProviderErrors() {
        assertTrue(ReleaseRules.shouldFallbackModel(429, "rate limit"));
        assertTrue(ReleaseRules.shouldFallbackModel(200, "provider unavailable"));
        assertFalse(ReleaseRules.shouldFallbackModel(401, "invalid api key"));
        assertFalse(ReleaseRules.shouldFallbackModel(400, "invalid JSON"));
    }

    @Test public void folderAndTagValuesAreBoundedAndNormalized() {
        assertEquals("My folder name", ReleaseRules.sanitizeOrganizationText(" My\nfolder\tname ", 80));
        assertEquals("rust,android", ReleaseRules.sanitizeTags("rust, android, rust;\n"));
        String many = "t1,t2,t3,t4,t5,t6,t7,t8,t9,t10,t11,t12,t13";
        assertEquals(12, ReleaseRules.sanitizeTags(many).split(",").length);
        assertEquals(32, ReleaseRules.sanitizeTags(repeat('a', 50)).length());
    }

    @Test public void attachmentTypesAreRestrictedToSupportedTextAndImageTypes() {
        assertTrue(ReleaseRules.isSupportedTextExtension("Rs"));
        assertTrue(ReleaseRules.isSupportedTextExtension("yaml"));
        assertFalse(ReleaseRules.isSupportedTextExtension("exe"));
        assertTrue(ReleaseRules.isSupportedImageMimeType("image/jpeg"));
        assertFalse(ReleaseRules.isSupportedImageMimeType("image/svg+xml"));
        assertFalse(ReleaseRules.isSupportedImageMimeType(null));
    }

    @Test public void visionDetectionIsHeuristicAndNullSafe() {
        assertTrue(ReleaseRules.isLikelyVisionModel("Gemini-2.5-Flash"));
        assertTrue(ReleaseRules.isLikelyVisionModel("llava-1.6"));
        assertFalse(ReleaseRules.isLikelyVisionModel("null"));
        assertFalse(ReleaseRules.isLikelyVisionModel(null));
    }

    private static String repeat(char ch, int count) {
        char[] chars = new char[count];
        Arrays.fill(chars, ch);
        return new String(chars);
    }
}
