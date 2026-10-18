package com.saarthi.studio;
import org.junit.Test;
import static org.junit.Assert.*;
public class BackupPolicyTest {
 @Test public void acceptsOnlySupportedLegacyAndCurrentSchemas() {
  assertTrue(BackupPolicy.isSupportedBackup(BackupPolicy.FORMAT,1));
  assertTrue(BackupPolicy.isSupportedBackup(BackupPolicy.FORMAT,2));
  assertTrue(BackupPolicy.isSupportedBackup(BackupPolicy.FORMAT,3));
  assertFalse(BackupPolicy.isSupportedBackup(BackupPolicy.FORMAT,0));
  assertFalse(BackupPolicy.isSupportedBackup(BackupPolicy.FORMAT,4));
  assertFalse(BackupPolicy.isSupportedBackup("other-format",3));
 }
 @Test public void enforcesBackupSizeWithBoundaryAndOverflowSafety() {
  assertTrue(BackupPolicy.isWithinSizeLimit(0));
  assertTrue(BackupPolicy.isWithinSizeLimit(BackupPolicy.MAX_BACKUP_BYTES));
  assertFalse(BackupPolicy.isWithinSizeLimit(BackupPolicy.MAX_BACKUP_BYTES+1L));
  assertFalse(BackupPolicy.isWithinSizeLimit(-1));
  assertFalse(BackupPolicy.isWithinSizeLimit(Long.MAX_VALUE));
 }
 @Test public void validatesImportedRoles() {
  assertTrue(BackupPolicy.isValidMessageRole("user"));
  assertTrue(BackupPolicy.isValidMessageRole("assistant"));
  assertFalse(BackupPolicy.isValidMessageRole("system"));
  assertFalse(BackupPolicy.isValidMessageRole(null));
 }
 @Test public void fingerprintIsStableAndChangesWhenBytesChange() {
  byte[] a="{\"schema_version\":3}".getBytes(java.nio.charset.StandardCharsets.UTF_8);
  byte[] b="{\"schema_version\":2}".getBytes(java.nio.charset.StandardCharsets.UTF_8);
  assertEquals(BackupPolicy.sha256(a),BackupPolicy.sha256(a.clone()));
  assertNotEquals(BackupPolicy.sha256(a),BackupPolicy.sha256(b));
  assertEquals(64,BackupPolicy.sha256(a).length());
 }

 @Test public void rejectsTooManyInlineImagesAndAggregateImageBytes() {
  String marker = "[[SAARTHI_IMAGE:image/png:iVBORw0KGgo=]]";
  assertFalse(BackupPolicy.containsMalformedImageMarkers(marker + "\n" + marker));
  assertTrue(BackupPolicy.containsMalformedImageMarkers(marker + "\n" + marker + "\n" + marker));
  String largePayload = java.util.Base64.getEncoder().encodeToString(new byte[460 * 1024]);
  String twoLarge = "[[SAARTHI_IMAGE:image/png:" + largePayload + "]]\n" +
          "[[SAARTHI_IMAGE:image/png:" + largePayload + "]]";
  assertTrue(BackupPolicy.containsMalformedImageMarkers(twoLarge));
 }
}
