package com.saarthi.studio;
import org.junit.Test;
import static org.junit.Assert.*;
public class AttachmentPolicyTest {
 @Test public void textExtensionsAndImageMimeTypesAreAllowlisted() {
  assertTrue(AttachmentPolicy.isSupportedTextExtension("Rs"));
  assertTrue(AttachmentPolicy.isSupportedTextExtension(".YAML"));
  assertFalse(AttachmentPolicy.isSupportedTextExtension("exe"));
  assertFalse(AttachmentPolicy.isSupportedTextExtension(null));
  assertTrue(AttachmentPolicy.isSupportedImageMimeType("image/jpeg"));
  assertTrue(AttachmentPolicy.isSupportedImageMimeType("image/webp"));
  assertFalse(AttachmentPolicy.isSupportedImageMimeType("image/svg+xml"));
  assertFalse(AttachmentPolicy.isSupportedImageMimeType(null));
  assertTrue(AttachmentPolicy.isValidStoredImageMarker("image/png", "iVBORw0KGgo="));
  assertFalse(AttachmentPolicy.isValidStoredImageMarker("image/svg+xml", "iVBORw0KGgo="));
  assertFalse(AttachmentPolicy.isValidStoredImageMarker("image/png", "not base64?!"));
 }
 @Test public void checksPerFileAndTotalBudgetsAtBoundaries() {
  assertTrue(AttachmentPolicy.canAddTextFile(2)); assertFalse(AttachmentPolicy.canAddTextFile(3));
  assertTrue(AttachmentPolicy.isTextFileWithinLimit(AttachmentPolicy.MAX_TEXT_FILE_BYTES));
  assertFalse(AttachmentPolicy.isTextFileWithinLimit(AttachmentPolicy.MAX_TEXT_FILE_BYTES+1L));
  assertTrue(AttachmentPolicy.isTextTotalWithinLimit(512*1024,512*1024));
  assertFalse(AttachmentPolicy.isTextTotalWithinLimit(1024*1024,1));
  assertTrue(AttachmentPolicy.canAddImage(1)); assertFalse(AttachmentPolicy.canAddImage(2));
  assertTrue(AttachmentPolicy.isImageWithinLimit(AttachmentPolicy.MAX_IMAGE_BYTES));
  assertFalse(AttachmentPolicy.isImageWithinLimit(AttachmentPolicy.MAX_IMAGE_BYTES+1L));
  assertTrue(AttachmentPolicy.isImageTotalWithinLimit(500*1024,400*1024));
  assertFalse(AttachmentPolicy.isImageTotalWithinLimit(900*1024,1));
  assertFalse(AttachmentPolicy.isImageTotalWithinLimit(Long.MAX_VALUE,1));
 }
}
