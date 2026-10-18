package com.saarthi.studio;

import android.content.Context;
import androidx.test.core.app.ApplicationProvider;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import org.junit.Test;
import org.junit.runner.RunWith;
import static org.junit.Assert.*;

@RunWith(AndroidJUnit4.class)
public class ChatHistoryPagingInstrumentedTest {
    @Test public void messageCountAndBoundedHistoryReturnLatestMessagesInChronologicalOrder() {
        Context context = ApplicationProvider.getApplicationContext();
        context.deleteDatabase("saarthi_history.sqlite3");
        ChatDatabase store = new ChatDatabase(context);
        try {
            long conversationId = store.createConversation("Paging test");
            for (int i = 0; i < 205; i++) {
                store.addMessage(conversationId, i % 2 == 0 ? "user" : "assistant", "message-" + i, "");
            }
            assertEquals(205, store.countMessages(conversationId));
            java.util.List<ChatDatabase.Message> firstPage = store.allMessages(conversationId, 150);
            assertEquals(150, firstPage.size());
            assertEquals("message-55", firstPage.get(0).content);
            assertEquals("message-204", firstPage.get(149).content);
            java.util.List<ChatDatabase.Message> fullWindow = store.allMessages(conversationId, 300);
            assertEquals(205, fullWindow.size());
            assertEquals("message-0", fullWindow.get(0).content);
            assertEquals("message-204", fullWindow.get(204).content);
        } finally {
            store.close();
            context.deleteDatabase("saarthi_history.sqlite3");
        }
    }
}
