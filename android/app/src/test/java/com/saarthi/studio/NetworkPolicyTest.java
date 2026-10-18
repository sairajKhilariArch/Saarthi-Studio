package com.saarthi.studio;
import org.junit.Test;
import static org.junit.Assert.*;
public class NetworkPolicyTest {
 @Test public void retriesOnlyTransientFailuresAndNeverAfterPartialOutput() {
  assertTrue(NetworkPolicy.shouldRetrySameModel(500,"server error",false));
  assertTrue(NetworkPolicy.shouldRetrySameModel(0,"Failed to connect",false));
  assertTrue(NetworkPolicy.shouldRetrySameModel(0,"Read timeout",false));
  assertFalse(NetworkPolicy.shouldRetrySameModel(401,"invalid API key",false));
  assertFalse(NetworkPolicy.shouldRetrySameModel(429,"rate limit",false));
  assertFalse(NetworkPolicy.shouldRetrySameModel(500,"timeout",true));
 }
 @Test public void fallbacksCoverQuotaButNotAuthAndStopAfterPartialOutput() {
  assertTrue(NetworkPolicy.shouldFallbackModel(429,"rate limit",false));
  assertTrue(NetworkPolicy.shouldFallbackModel(500,"server error",false));
  assertTrue(NetworkPolicy.shouldFallbackModel(200,"provider unavailable",false));
  assertFalse(NetworkPolicy.shouldFallbackModel(401,"invalid API key",false));
  assertFalse(NetworkPolicy.shouldFallbackModel(500,"server error",true));
 }
 @Test public void diagnosticsRedactBearerAndApiKeyAndBoundLength() {
  String safe=NetworkPolicy.redactDiagnostic("Authorization: Bearer abcdef123456 api_key=verySecretKey","verySecretKey");
  assertFalse(safe.contains("abcdef123456")); assertFalse(safe.contains("verySecretKey")); assertTrue(safe.contains("[redacted]"));
  char[] chars=new char[400]; java.util.Arrays.fill(chars,'x');
  assertTrue(NetworkPolicy.redactDiagnostic(new String(chars),null).length()<=260);
 }
}
