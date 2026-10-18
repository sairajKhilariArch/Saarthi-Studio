package com.saarthi.studio;
import org.junit.Test;
import static org.junit.Assert.*;
public class ConversationSearchTest {
 @Test public void emptyQuerySignalsAllConversations() {
  assertTrue(ConversationSearch.fromText(null).isEmpty());
  assertTrue(ConversationSearch.fromText("  ").isEmpty());
  assertFalse(ConversationSearch.fromText("rust").isEmpty());
 }
 @Test public void queryBindsSearchTermToTitleFolderTagsAndMessageContent() {
  ConversationSearch.Query q=ConversationSearch.fromText(" Rust lifetimes ");
  assertEquals("Rust lifetimes",q.term()); assertEquals(5,q.arguments(250).length);
  assertTrue(q.sql().contains("EXISTS (SELECT 1 FROM messages")); assertFalse(q.sql().contains("Rust lifetimes"));
  for(int i=0;i<4;i++) assertEquals(q.term(),q.arguments(250)[i]);
  assertEquals("250",q.arguments(250)[4]);
 }
 @Test public void folderTagPinnedAndFavoriteFiltersAreBoundAndComposedWithSearch() {
  ConversationSearch.Query folder = ConversationSearch.fromTextAndFilter("rust", "Folder: Rust & College");
  assertTrue(folder.sql().contains("c.folder=?"));
  assertEquals("Rust & College", folder.arguments(250)[4]);
  assertEquals("250", folder.arguments(250)[5]);

  ConversationSearch.Query tag = ConversationSearch.fromTextAndFilter("", "Tag: android");
  assertTrue(tag.sql().contains("instr(',' || lower(c.tags)"));
  assertEquals("android", tag.arguments(250)[0]);
  assertEquals("250", tag.arguments(250)[1]);
  assertTrue(ConversationSearch.fromTextAndFilter("", "Pinned only").sql().contains("c.is_pinned=1"));
  assertTrue(ConversationSearch.fromTextAndFilter("", "Favorites only").sql().contains("c.is_favorite=1"));
  assertTrue(ConversationSearch.fromTextAndFilter("lifetimes", "Pinned only").sql().contains("AND c.is_pinned=1"));
 }
 @Test public void previewNormalizesWhitespaceAndAvoidsFalseMatch() {
  assertEquals("line one line two",ConversationSearch.matchingPreview(" line one \n line two ","line"));
  assertEquals("",ConversationSearch.matchingPreview("text","missing"));
  assertEquals("",ConversationSearch.matchingPreview("text",""));
 }
}
