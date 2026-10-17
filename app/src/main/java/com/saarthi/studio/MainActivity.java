package com.saarthi.studio;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Context;
import android.content.Intent;
import android.content.SharedPreferences;
import android.graphics.Typeface;
import android.graphics.Color;
import android.graphics.drawable.GradientDrawable;
import android.net.Uri;
import android.os.Bundle;
import android.os.Handler;
import android.os.Looper;
import android.text.InputType;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputMethodManager;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.ScrollView;
import android.widget.TextView;
import android.widget.Toast;

import org.json.JSONArray;
import org.json.JSONException;
import org.json.JSONObject;

import java.io.ByteArrayOutputStream;
import java.io.InputStream;
import java.io.OutputStream;
import java.net.HttpURLConnection;
import java.net.URL;
import java.nio.charset.StandardCharsets;
import java.security.KeyStore;
import java.util.ArrayList;
import java.util.Collections;
import java.util.List;
import java.util.Locale;
import java.util.concurrent.ExecutorService;
import java.util.concurrent.Executors;

import javax.crypto.Cipher;
import javax.crypto.KeyGenerator;
import javax.crypto.SecretKey;
import javax.crypto.spec.GCMParameterSpec;
import android.security.keystore.KeyGenParameterSpec;
import android.security.keystore.KeyProperties;
import android.util.Base64;

public class MainActivity extends Activity {
    private static final int BG = Color.rgb(16, 19, 26);
    private static final int PANEL = Color.rgb(27, 32, 43);
    private static final int PANEL_ALT = Color.rgb(35, 42, 55);
    private static final int TEXT = Color.rgb(238, 242, 248);
    private static final int MUTED = Color.rgb(155, 166, 184);
    private static final int GREEN = Color.rgb(118, 215, 165);
    private static final int USER_BUBBLE = Color.rgb(39, 64, 60);
    private static final String SYSTEM_PROMPT = "You are a concise, helpful coding assistant. Be accurate and safe; never claim to run code unless you did.";
    private static final String KEY_ALIAS = "SaarthiStudioSettingsKeyV1";

    private final ExecutorService executor = Executors.newSingleThreadExecutor();
    private final Handler main = new Handler(Looper.getMainLooper());
    private Store store;
    private SecureSettings secureSettings;
    private Config config;
    private LinearLayout root, messageList;
    private ScrollView scroll;
    private EditText promptInput, codeInput;
    private Button sendButton, codeButton, historyButton, newChatButton;
    private TextView statusText, usageText, currentChatText;
    private boolean codeVisible = false;
    private boolean sending = false;
    private long currentConversationId = -1L;
    private static final String STATE_PREFS = "saarthi_chat_state";
    private static final String CURRENT_CONVERSATION_KEY = "current_conversation_id";

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().setStatusBarColor(BG);
        getWindow().setNavigationBarColor(BG);
        store = new Store(this);
        secureSettings = new SecureSettings(this);
        config = secureSettings.load();
        restoreCurrentConversation();
        buildUi();
        renderConversationTitle();
        renderHistory();
        if (config.apiKey.isEmpty() || config.baseUrl.isEmpty()) {
            statusText.setText("Setup needed · connect to FreeLLMAPI");
        } else {
            statusText.setText("Ready to connect · " + config.model);
        }
    }

    @Override protected void onDestroy() {
        super.onDestroy();
        executor.shutdownNow();
    }

    private void buildUi() {
        root = new LinearLayout(this);
        root.setOrientation(LinearLayout.VERTICAL);
        root.setBackgroundColor(BG);
        root.setPadding(dp(16), dp(12), dp(16), dp(10));
        setContentView(root);

        LinearLayout header = row();
        header.setGravity(Gravity.CENTER_VERTICAL);
        TextView logo = label("S", 23, GREEN, true);
        logo.setGravity(Gravity.CENTER);
        logo.setBackground(round(PANEL_ALT, 16));
        header.addView(logo, new LinearLayout.LayoutParams(dp(48), dp(48)));
        LinearLayout brand = new LinearLayout(this);
        brand.setOrientation(LinearLayout.VERTICAL);
        brand.setPadding(dp(11), 0, 0, 0);
        brand.addView(label("Saarthi Studio", 20, TEXT, true));
        TextView tagline = label("One workspace. Many providers.", 12, MUTED, false);
        brand.addView(tagline);
        header.addView(brand, new LinearLayout.LayoutParams(0, -2, 1));
        historyButton = smallButton("Chats");
        historyButton.setOnClickListener(v -> openConversationHistory());
        header.addView(historyButton);
        Button settings = smallButton("Settings");
        settings.setOnClickListener(v -> openSettings());
        LinearLayout.LayoutParams settingsLp = new LinearLayout.LayoutParams(-2, -2);
        settingsLp.leftMargin = dp(6);
        header.addView(settings, settingsLp);
        root.addView(header);

        LinearLayout chatBar = row();
        chatBar.setGravity(Gravity.CENTER_VERTICAL);
        chatBar.setPadding(dp(2), dp(9), dp(2), dp(5));
        currentChatText = label("New chat", 14, TEXT, true);
        currentChatText.setSingleLine(true);
        currentChatText.setEllipsize(android.text.TextUtils.TruncateAt.END);
        chatBar.addView(currentChatText, new LinearLayout.LayoutParams(0, -2, 1));
        newChatButton = smallButton("+ New chat");
        newChatButton.setOnClickListener(v -> createNewConversation());
        chatBar.addView(newChatButton);
        root.addView(chatBar);

        statusText = label("Not connected", 12, MUTED, false);
        statusText.setPadding(dp(4), dp(5), dp(4), dp(8));
        root.addView(statusText);
        View line = new View(this); line.setBackgroundColor(Color.rgb(44, 51, 65));
        root.addView(line, new LinearLayout.LayoutParams(-1, dp(1)));

        scroll = new ScrollView(this);
        scroll.setFillViewport(true);
        scroll.setClipToPadding(false);
        scroll.setPadding(0, dp(12), 0, dp(12));
        messageList = new LinearLayout(this);
        messageList.setOrientation(LinearLayout.VERTICAL);
        scroll.addView(messageList, new ScrollView.LayoutParams(-1, -2));
        LinearLayout.LayoutParams scrollLp = new LinearLayout.LayoutParams(-1, 0, 1);
        scrollLp.topMargin = dp(3); scrollLp.bottomMargin = dp(8);
        root.addView(scroll, scrollLp);

        codeInput = new EditText(this);
        codeInput.setTextColor(TEXT); codeInput.setHintTextColor(MUTED);
        codeInput.setHint("Paste code here for review or debugging…");
        codeInput.setTextSize(13); codeInput.setGravity(Gravity.TOP | Gravity.START);
        codeInput.setTypeface(Typeface.MONOSPACE);
        codeInput.setMinLines(4); codeInput.setMaxLines(9);
        codeInput.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE | InputType.TYPE_TEXT_FLAG_CAP_SENTENCES);
        codeInput.setBackground(round(PANEL, 12)); codeInput.setPadding(dp(12), dp(10), dp(12), dp(10));
        codeInput.setVisibility(View.GONE);
        LinearLayout.LayoutParams codeLp = new LinearLayout.LayoutParams(-1, -2);
        codeLp.bottomMargin = dp(8); root.addView(codeInput, codeLp);

        LinearLayout toolRow = row();
        codeButton = smallButton("＋ Code context");
        codeButton.setOnClickListener(v -> {
            codeVisible = !codeVisible;
            codeInput.setVisibility(codeVisible ? View.VISIBLE : View.GONE);
            codeButton.setText(codeVisible ? "− Hide code" : "＋ Code context");
        });
        toolRow.addView(codeButton);
        usageText = label("Saved on this phone", 11, MUTED, false);
        usageText.setGravity(Gravity.CENTER_VERTICAL | Gravity.RIGHT);
        LinearLayout.LayoutParams usageLp = new LinearLayout.LayoutParams(0, -2, 1);
        toolRow.addView(usageText, usageLp);
        root.addView(toolRow);

        LinearLayout composer = row();
        composer.setGravity(Gravity.BOTTOM);
        promptInput = new EditText(this);
        promptInput.setTextColor(TEXT); promptInput.setHintTextColor(MUTED);
        promptInput.setHint("Message Saarthi…"); promptInput.setTextSize(15);
        promptInput.setMinLines(1); promptInput.setMaxLines(4);
        promptInput.setGravity(Gravity.TOP | Gravity.START);
        promptInput.setInputType(InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_MULTI_LINE | InputType.TYPE_TEXT_FLAG_CAP_SENTENCES);
        promptInput.setImeOptions(EditorInfo.IME_ACTION_SEND);
        promptInput.setBackground(round(PANEL, 16));
        promptInput.setPadding(dp(13), dp(12), dp(13), dp(12));
        composer.addView(promptInput, new LinearLayout.LayoutParams(0, -2, 1));
        sendButton = new Button(this); sendButton.setText("Send"); sendButton.setTextColor(BG);
        sendButton.setAllCaps(false); sendButton.setTextSize(14); sendButton.setTypeface(null, Typeface.BOLD);
        sendButton.setBackground(round(GREEN, 14));
        LinearLayout.LayoutParams sendLp = new LinearLayout.LayoutParams(dp(82), dp(49));
        sendLp.leftMargin = dp(8); composer.addView(sendButton, sendLp);
        root.addView(composer);
        sendButton.setOnClickListener(v -> sendMessage());
        promptInput.setOnEditorActionListener((v, actionId, event) -> {
            if (actionId == EditorInfo.IME_ACTION_SEND) { sendMessage(); return true; }
            return false;
        });
    }

    private void renderHistory() {
        messageList.removeAllViews();
        List<Message> history = store.allMessages(currentConversationId, 150);
        if (history.isEmpty()) {
            LinearLayout welcome = new LinearLayout(this);
            welcome.setOrientation(LinearLayout.VERTICAL);
            welcome.setPadding(dp(18), dp(18), dp(18), dp(18));
            welcome.setBackground(round(PANEL, 18));
            TextView title = label("Welcome to Saarthi Studio", 20, TEXT, true);
            welcome.addView(title);
            TextView body = label("This is a separate conversation. Connect FreeLLMAPI in Settings, then ask questions, debug code, or explore ideas. Your chats stay on this phone.", 14, MUTED, false);
            body.setPadding(0, dp(9), 0, 0); welcome.addView(body);
            TextView steps = label("Quick start\n1. Run FreeLLMAPI on your Ubuntu computer or server.\n2. Enter its network URL and unified API key in Settings.\n3. Tap Test connection, then start chatting.\n\nUse Chats to switch between saved conversations, or + New chat to start another.", 13, TEXT, false);
            steps.setPadding(0, dp(15), 0, 0); welcome.addView(steps);
            messageList.addView(welcome, new LinearLayout.LayoutParams(-1, -2));
        } else {
            for (Message m : history) addBubble(m.role, m.content, m.meta, false);
        }
        scrollToBottom();
    }

    private void addBubble(String role, String content, String meta, boolean scrollNow) {
        boolean user = "user".equals(role);
        LinearLayout wrap = new LinearLayout(this);
        wrap.setOrientation(LinearLayout.VERTICAL);
        wrap.setGravity(user ? Gravity.RIGHT : Gravity.LEFT);
        LinearLayout.LayoutParams wrapLp = new LinearLayout.LayoutParams(-1, -2);
        wrapLp.bottomMargin = dp(13);
        messageList.addView(wrap, wrapLp);
        TextView who = label(user ? "YOU" : "SAARTHI", 10, user ? GREEN : MUTED, true);
        who.setPadding(dp(4), 0, dp(4), dp(5));
        wrap.addView(who);
        TextView bubble = label(content, 15, TEXT, false);
        bubble.setTextIsSelectable(true);
        bubble.setPadding(dp(13), dp(11), dp(13), dp(11));
        bubble.setBackground(round(user ? USER_BUBBLE : PANEL, 16));
        LinearLayout.LayoutParams bp = new LinearLayout.LayoutParams(-1, -2);
        bp.leftMargin = user ? dp(28) : 0;
        bp.rightMargin = user ? 0 : dp(18);
        wrap.addView(bubble, bp);
        if (meta != null && !meta.isEmpty()) {
            TextView m = label(meta, 10, MUTED, false);
            m.setPadding(dp(4), dp(5), dp(4), 0); wrap.addView(m);
        }
        if (scrollNow) scrollToBottom();
    }

    private void sendMessage() {
        if (sending) return;
        String prompt = promptInput.getText().toString().trim();
        if (prompt.isEmpty()) { promptInput.setError("Type a message first"); return; }
        if (config.baseUrl.trim().isEmpty() || config.apiKey.trim().isEmpty()) {
            openSettings(); toast("Set the FreeLLMAPI URL and unified API key first."); return;
        }
        String code = codeVisible ? codeInput.getText().toString().trim() : "";
        String fullText = prompt + (code.isEmpty() ? "" : "\n\n[Code context]\n" + code);
        final long targetConversationId = currentConversationId;
        store.renameIfDefaultTitle(targetConversationId, promptToTitle(prompt));
        renderConversationTitle();
        store.addMessage(targetConversationId, "user", fullText, "");
        messageList.removeAllViews();
        for (Message m : store.allMessages(targetConversationId, 150)) addBubble(m.role, m.content, m.meta, false);
        promptInput.setText("");
        ((InputMethodManager)getSystemService(INPUT_METHOD_SERVICE)).hideSoftInputFromWindow(promptInput.getWindowToken(), 0);
        setSending(true);
        statusText.setText("Waiting for provider response…");
        usageText.setText("Sending latest 4 messages");

        List<Message> context = store.latestMessages(targetConversationId, 4);
        JSONArray msgs = new JSONArray();
        try { msgs.put(new JSONObject().put("role", "system").put("content", SYSTEM_PROMPT)); }
        catch (JSONException ignored) { }
        for (Message m : context) {
            try { msgs.put(new JSONObject().put("role", m.role).put("content", m.content)); }
            catch (JSONException ignored) { }
        }
        String base = config.baseUrl;
        String key = config.apiKey;
        String model = config.model == null || config.model.trim().isEmpty() ? "auto" : config.model.trim();
        executor.execute(() -> {
            long started = System.currentTimeMillis();
            try {
                JSONObject body = new JSONObject(); body.put("model", model); body.put("messages", msgs); body.put("stream", false);
                HttpResult result = request("POST", endpoint(base, "chat/completions"), key, body.toString());
                JSONObject json = parseJson(result.body);
                if (result.status < 200 || result.status >= 300) throw new Exception(errorText(json, result.body, result.status));
                String answer = responseText(json);
                if (answer.trim().isEmpty()) throw new Exception("Router returned an empty response.");
                JSONObject usage = json.optJSONObject("usage");
                String tokenText = usage == null ? "token count not reported" : (usage.opt("total_tokens") == null ? "token count not reported" : usage.opt("total_tokens") + " tokens");
                String provider = result.headers.getOrDefault("x-routed-via", "provider not reported");
                long ms = System.currentTimeMillis() - started;
                String meta = tokenText + " · " + String.format(Locale.US, "%.1f s", ms / 1000.0) + " · " + provider;
                store.addMessage(targetConversationId, "assistant", answer, meta);
                main.post(() -> {
                    setSending(false); renderHistory(); statusText.setText("Connected · " + model); usageText.setText(meta); codeInput.setText("");
                });
            } catch (Exception e) {
                String message = friendlyError(e.getMessage());
                long ms = System.currentTimeMillis() - started;
                String meta = "Failed after " + String.format(Locale.US, "%.1f s", ms / 1000.0);
                store.addMessage(targetConversationId, "assistant", message, meta);
                main.post(() -> { setSending(false); renderHistory(); statusText.setText("Request failed · check Settings or server logs"); usageText.setText(meta); });
            }
        });
    }

    private void setSending(boolean value) {
        sending = value;
        sendButton.setEnabled(!value);
        sendButton.setText(value ? "…" : "Send");
        sendButton.setAlpha(value ? 0.6f : 1f);
    }

    private void restoreCurrentConversation() {
        android.content.SharedPreferences state = getSharedPreferences(STATE_PREFS, MODE_PRIVATE);
        long savedId = state.getLong(CURRENT_CONVERSATION_KEY, -1L);
        Conversation saved = store.getConversation(savedId);
        if (saved != null) {
            currentConversationId = saved.id;
            return;
        }
        List<Conversation> conversations = store.allConversations();
        if (conversations.isEmpty()) {
            currentConversationId = store.createConversation("New chat");
        } else {
            // On an existing installation, this selects the migrated legacy conversation.
            currentConversationId = conversations.get(0).id;
        }
        saveCurrentConversationId();
    }

    private void saveCurrentConversationId() {
        getSharedPreferences(STATE_PREFS, MODE_PRIVATE).edit()
                .putLong(CURRENT_CONVERSATION_KEY, currentConversationId).apply();
    }

    private void renderConversationTitle() {
        if (currentChatText == null || store == null) return;
        Conversation conversation = store.getConversation(currentConversationId);
        currentChatText.setText(conversation == null ? "New chat" : conversation.title);
    }

    private void createNewConversation() {
        if (sending) { toast("Wait for the current response to finish before switching chats."); return; }
        currentConversationId = store.createConversation("New chat");
        saveCurrentConversationId();
        renderConversationTitle();
        renderHistory();
        promptInput.setText("");
        codeInput.setText("");
        usageText.setText("New conversation · saved on this phone");
    }

    private void openConversationHistory() {
        if (sending) { toast("Wait for the current response to finish before switching chats."); return; }
        LinearLayout list = new LinearLayout(this);
        list.setOrientation(LinearLayout.VERTICAL);
        list.setPadding(dp(14), dp(8), dp(14), dp(8));
        ScrollView historyScroll = new ScrollView(this);
        historyScroll.setFillViewport(false);
        historyScroll.addView(list);
        AlertDialog dialog = new AlertDialog.Builder(this)
                .setTitle("Your conversations")
                .setView(historyScroll)
                .setNegativeButton("Close", (d, w) -> d.dismiss())
                .setPositiveButton("+ New chat", (d, w) -> createNewConversation())
                .create();
        Runnable[] refreshHolder = new Runnable[1];
        refreshHolder[0] = () -> {
            list.removeAllViews();
            List<Conversation> conversations = store.allConversations();
            if (conversations.isEmpty()) {
                TextView empty = label("No saved conversations yet. Create a new chat to begin.", 14, MUTED, false);
                empty.setPadding(dp(8), dp(12), dp(8), dp(12));
                list.addView(empty);
                return;
            }
            for (Conversation conversation : conversations) {
                LinearLayout card = new LinearLayout(this);
                card.setOrientation(LinearLayout.VERTICAL);
                card.setBackground(round(conversation.id == currentConversationId ? PANEL_ALT : PANEL, 14));
                card.setPadding(dp(12), dp(10), dp(12), dp(10));
                LinearLayout.LayoutParams cardLp = new LinearLayout.LayoutParams(-1, -2);
                cardLp.bottomMargin = dp(9);
                list.addView(card, cardLp);
                TextView title = label(conversation.title, 15, TEXT, true);
                card.addView(title);
                String preview = store.preview(conversation.id);
                TextView detail = label(preview.isEmpty() ? "No messages yet" : preview, 12, MUTED, false);
                detail.setMaxLines(2);
                detail.setPadding(0, dp(4), 0, dp(9));
                card.addView(detail);
                LinearLayout actions = row();
                actions.setGravity(Gravity.RIGHT | Gravity.CENTER_VERTICAL);
                Button open = smallButton(conversation.id == currentConversationId ? "Current" : "Open");
                open.setEnabled(conversation.id != currentConversationId);
                open.setOnClickListener(v -> {
                    currentConversationId = conversation.id;
                    saveCurrentConversationId();
                    renderConversationTitle();
                    renderHistory();
                    usageText.setText("Conversation loaded · saved on this phone");
                    dialog.dismiss();
                });
                Button rename = smallButton("Rename");
                LinearLayout.LayoutParams actionLp = new LinearLayout.LayoutParams(-2, -2);
                actionLp.leftMargin = dp(6);
                actions.addView(open);
                actions.addView(rename, actionLp);
                Button delete = smallButton("Delete");
                delete.setTextColor(Color.rgb(255, 145, 145));
                LinearLayout.LayoutParams deleteLp = new LinearLayout.LayoutParams(-2, -2);
                deleteLp.leftMargin = dp(6);
                actions.addView(delete, deleteLp);
                card.addView(actions);
                rename.setOnClickListener(v -> promptRenameConversation(conversation, () -> {
                    refreshHolder[0].run(); renderConversationTitle();
                }));
                delete.setOnClickListener(v -> confirmDeleteConversation(conversation, () -> {
                    if (conversation.id == currentConversationId) {
                        List<Conversation> remaining = store.allConversations();
                        currentConversationId = remaining.isEmpty() ? store.createConversation("New chat") : remaining.get(0).id;
                        saveCurrentConversationId();
                        renderConversationTitle();
                        renderHistory();
                    }
                    refreshHolder[0].run();
                }));
            }
        };
        refreshHolder[0].run();
        dialog.setOnShowListener(ignored -> {
            dialog.getButton(AlertDialog.BUTTON_POSITIVE).setTextColor(GREEN);
            dialog.getButton(AlertDialog.BUTTON_NEGATIVE).setTextColor(MUTED);
        });
        dialog.show();
        dialog.getWindow().setLayout(Math.min(getResources().getDisplayMetrics().widthPixels - dp(24), dp(520)), Math.min(getResources().getDisplayMetrics().heightPixels - dp(100), dp(560)));
    }

    private void promptRenameConversation(Conversation conversation, Runnable onChanged) {
        EditText input = field("Conversation name", conversation.title, InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_FLAG_CAP_SENTENCES);
        new AlertDialog.Builder(this)
                .setTitle("Rename conversation")
                .setView(input)
                .setNegativeButton("Cancel", null)
                .setPositiveButton("Save", (d, w) -> {
                    String title = input.getText().toString().trim();
                    if (title.isEmpty()) { toast("Conversation name cannot be empty."); return; }
                    store.renameConversation(conversation.id, title);
                    onChanged.run();
                })
                .show();
    }

    private void confirmDeleteConversation(Conversation conversation, Runnable onDeleted) {
        new AlertDialog.Builder(this)
                .setTitle("Delete conversation?")
                .setMessage("Delete ‘" + conversation.title + "’ and all its messages from this phone? This cannot be undone.")
                .setNegativeButton("Cancel", null)
                .setPositiveButton("Delete", (d, w) -> {
                    store.deleteConversation(conversation.id);
                    onDeleted.run();
                    toast("Conversation deleted.");
                })
                .show();
    }

    private static String promptToTitle(String prompt) {
        String firstLine = prompt == null ? "" : prompt.trim().split("\n", 2)[0].replace("\r", "").trim();
        if (firstLine.isEmpty()) return "New chat";
        if (firstLine.length() > 38) return firstLine.substring(0, 35).trim() + "…";
        return firstLine;
    }

    private void openSettings() {
        LinearLayout form = new LinearLayout(this);
        form.setPadding(dp(20), dp(10), dp(20), dp(4));
        form.setOrientation(LinearLayout.VERTICAL);
        form.setBackgroundColor(PANEL);
        TextView help = label("Point Saarthi to your FreeLLMAPI instance. On the same Wi-Fi, use your Ubuntu computer's LAN IP—not localhost.", 13, MUTED, false);
        help.setPadding(0, 0, 0, dp(12)); form.addView(help);
        EditText base = field("Base URL, e.g. http://192.168.1.10:3001/v1", config.baseUrl, InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_URI);
        form.addView(base, fieldLp());
        EditText model = field("Model", config.model, InputType.TYPE_CLASS_TEXT);
        form.addView(model, fieldLp());
        EditText key = field("Unified API key (leave blank to keep saved key)", "", InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD);
        form.addView(key, fieldLp());
        Button test = smallButton("Test connection");
        test.setTextColor(GREEN);
        LinearLayout.LayoutParams testLp = new LinearLayout.LayoutParams(-1, dp(44));
        testLp.bottomMargin = dp(10);
        form.addView(test, testLp);
        test.setOnClickListener(v -> testConnection(base, key));
        TextView security = label("Security: HTTP is suitable only for a trusted local network. Use HTTPS for a public server. The key is encrypted with Android Keystore on this phone.", 12, MUTED, false);
        security.setPadding(0, dp(2), 0, dp(7)); form.addView(security);
        TextView dash = label("Provider dashboard opens at the server address (without /v1). Create/copy your unified key there.", 12, MUTED, false);
        dash.setPadding(0, 0, 0, dp(5)); form.addView(dash);
        ScrollView sc = new ScrollView(this); sc.addView(form);
        AlertDialog dialog = new AlertDialog.Builder(this)
                .setTitle("Router settings")
                .setView(sc)
                .setNegativeButton("Close", (d, w) -> d.dismiss())
                .setNeutralButton("Open dashboard", null)
                .setPositiveButton("Save settings", null)
                .create();
        dialog.setOnShowListener(ignored -> {
            dialog.getButton(AlertDialog.BUTTON_POSITIVE).setOnClickListener(v -> {
                String url = base.getText().toString().trim().replaceAll("/+$", "");
                String mod = model.getText().toString().trim(); if (mod.isEmpty()) mod = "auto";
                String newKey = key.getText().toString().trim();
                if (!validUrl(url)) { base.setError("Use a valid http:// or https:// URL ending in /v1"); return; }
                if (!newKey.isEmpty()) config.apiKey = newKey;
                config.baseUrl = url; config.model = mod;
                try {
                    secureSettings.save(config);
                    statusText.setText(config.apiKey.isEmpty() ? "API key needed" : "Settings saved · ready to test");
                    toast("Settings saved securely on this phone."); dialog.dismiss();
                } catch (Exception ex) { toast("Could not save settings: " + ex.getMessage()); }
            });
            dialog.getButton(AlertDialog.BUTTON_NEUTRAL).setOnClickListener(v -> openDashboard(base.getText().toString().trim()));
        });
        dialog.show();
        dialog.getWindow().setLayout(Math.min(getResources().getDisplayMetrics().widthPixels - dp(24), dp(520)), ViewGroup.LayoutParams.WRAP_CONTENT);
        dialog.getWindow().setSoftInputMode(android.view.WindowManager.LayoutParams.SOFT_INPUT_ADJUST_RESIZE);
    }

    private void openDashboard(String base) {
        try {
            String url = base.trim().replaceAll("/+$", "").replaceFirst("/v1$", "");
            if (!url.startsWith("http://") && !url.startsWith("https://")) { toast("Save a valid server URL first."); return; }
            startActivity(new Intent(Intent.ACTION_VIEW, Uri.parse(url)));
        } catch (Exception e) { toast("Could not open dashboard."); }
    }

    private static boolean validUrl(String url) {
        if (!(url.startsWith("http://") || url.startsWith("https://"))) return false;
        try { Uri u = Uri.parse(url); return !u.getHost().isEmpty() && url.endsWith("/v1"); }
        catch (Exception e) { return false; }
    }

    private void testConnection(EditText base, EditText key) {
        String url = base.getText().toString().trim().replaceAll("/+$", "");
        String k = key.getText().toString().trim().isEmpty() ? config.apiKey : key.getText().toString().trim();
        if (!validUrl(url)) { base.setError("Use a valid server URL ending in /v1"); return; }
        if (k.isEmpty()) { key.setError("Enter your unified API key"); return; }
        toast("Testing server…");
        executor.execute(() -> {
            try {
                HttpResult r = request("GET", endpoint(url, "models"), k, null);
                if (r.status >= 200 && r.status < 300) main.post(() -> toast("Connection successful. FreeLLMAPI is reachable."));
                else main.post(() -> toast("Server responded HTTP " + r.status + ". Check URL/key; /models may not be enabled."));
            } catch (Exception e) { main.post(() -> toast("Could not reach server: " + friendlyError(e.getMessage()))); }
        });
    }

    private static HttpResult request(String method, String url, String apiKey, String body) throws Exception {
        HttpURLConnection conn = (HttpURLConnection)new URL(url).openConnection();
        conn.setRequestMethod(method); conn.setConnectTimeout(12000); conn.setReadTimeout(120000);
        conn.setRequestProperty("Accept", "application/json");
        if (apiKey != null && !apiKey.isEmpty()) conn.setRequestProperty("Authorization", "Bearer " + apiKey);
        if (body != null) {
            conn.setDoOutput(true); conn.setRequestProperty("Content-Type", "application/json; charset=utf-8");
            try (OutputStream out = conn.getOutputStream()) { out.write(body.getBytes(StandardCharsets.UTF_8)); }
        }
        int status = conn.getResponseCode();
        InputStream stream = status >= 400 ? conn.getErrorStream() : conn.getInputStream();
        String text = stream == null ? "" : readFully(stream);
        java.util.Map<String, String> headers = new java.util.HashMap<>();
        String via = conn.getHeaderField("x-routed-via"); if (via != null) headers.put("x-routed-via", via);
        conn.disconnect(); return new HttpResult(status, text, headers);
    }

    private static String endpoint(String base, String path) { return base.replaceAll("/+$", "") + "/" + path.replaceAll("^/+", ""); }
    private static String readFully(InputStream input) throws Exception {
        try (InputStream in = input; ByteArrayOutputStream out = new ByteArrayOutputStream()) {
            byte[] b = new byte[4096]; int n; while ((n = in.read(b)) != -1) out.write(b, 0, n);
            return out.toString(StandardCharsets.UTF_8.name());
        }
    }
    private static JSONObject parseJson(String body) { try { return new JSONObject(body); } catch (Exception e) { return new JSONObject(); } }
    private static String responseText(JSONObject data) throws JSONException {
        JSONObject choice = data.optJSONArray("choices") == null ? null : data.optJSONArray("choices").optJSONObject(0);
        JSONObject message = choice == null ? null : choice.optJSONObject("message");
        if (message == null) return data.toString(2);
        Object content = message.opt("content");
        if (content instanceof String) return (String)content;
        if (content instanceof JSONArray) {
            JSONArray arr = (JSONArray)content; StringBuilder b = new StringBuilder();
            for (int i=0; i<arr.length(); i++) { JSONObject part = arr.optJSONObject(i); if (part != null && part.has("text")) { if (b.length()>0) b.append('\n'); b.append(part.optString("text")); } }
            if (b.length()>0) return b.toString();
        }
        return content == null ? data.toString(2) : content.toString();
    }
    private static String errorText(JSONObject json, String raw, int status) {
        JSONObject error = json.optJSONObject("error");
        String msg = error == null ? json.optString("message", "") : error.optString("message", error.toString());
        if (msg.isEmpty()) msg = raw == null || raw.isEmpty() ? "Request failed" : raw.substring(0, Math.min(500, raw.length()));
        return "FreeLLMAPI returned HTTP " + status + ": " + msg;
    }
    private static String friendlyError(String message) {
        if (message == null || message.trim().isEmpty()) return "Unknown network error.";
        if (message.contains("Unable to resolve host") || message.contains("Failed to connect") || message.contains("timeout"))
            return "Cannot reach FreeLLMAPI. Check the phone and server are on the same network, use the Ubuntu LAN IP (not localhost), and confirm port 3001 is reachable. Details: " + message;
        return message;
    }

    private void scrollToBottom() { if (scroll != null) scroll.post(() -> scroll.fullScroll(View.FOCUS_DOWN)); }
    private TextView label(String text, int sp, int color, boolean bold) {
        TextView t = new TextView(this); t.setText(text); t.setTextSize(sp); t.setTextColor(color);
        if (bold) t.setTypeface(Typeface.DEFAULT, Typeface.BOLD); t.setLineSpacing(dp(2), 1f); return t;
    }
    private LinearLayout row() { LinearLayout l = new LinearLayout(this); l.setOrientation(LinearLayout.HORIZONTAL); return l; }
    private Button smallButton(String text) {
        Button b = new Button(this); b.setText(text); b.setTextSize(12); b.setAllCaps(false); b.setTextColor(TEXT); b.setBackground(round(PANEL_ALT, 12));
        b.setPadding(dp(8), 0, dp(8), 0); return b;
    }
    private EditText field(String hint, String value, int type) {
        EditText e = new EditText(this); e.setSingleLine(true); e.setTextSize(14); e.setTextColor(TEXT); e.setHintTextColor(MUTED);
        e.setHint(hint); e.setText(value == null ? "" : value); e.setInputType(type); e.setSelectAllOnFocus(false);
        e.setBackground(round(PANEL_ALT, 12)); e.setPadding(dp(12), dp(10), dp(12), dp(10)); return e;
    }
    private LinearLayout.LayoutParams fieldLp() { LinearLayout.LayoutParams p = new LinearLayout.LayoutParams(-1, -2); p.bottomMargin = dp(10); return p; }
    private GradientDrawable round(int color, int radius) { GradientDrawable d = new GradientDrawable(); d.setColor(color); d.setCornerRadius(dp(radius)); return d; }
    private int dp(float value) { return (int)(value * getResources().getDisplayMetrics().density + 0.5f); }
    private void toast(String msg) { Toast.makeText(this, msg, Toast.LENGTH_LONG).show(); }

    private static final class Config {
        String baseUrl = ""; String model = "auto"; String apiKey = "";
        JSONObject toJson() throws JSONException { JSONObject j = new JSONObject(); j.put("base_url", baseUrl); j.put("model", model); j.put("api_key", apiKey); return j; }
        static Config fromJson(JSONObject j) { Config c = new Config(); c.baseUrl=j.optString("base_url", ""); c.model=j.optString("model", "auto"); c.apiKey=j.optString("api_key", ""); return c; }
    }
    private static final class HttpResult {
        final int status; final String body; final java.util.Map<String,String> headers;
        HttpResult(int s, String b, java.util.Map<String,String> h) { status=s; body=b; headers=h; }
    }
    private static final class Message {
        final String role, content, meta;
        Message(String r, String c, String m) { role=r; content=c; meta=m == null ? "" : m; }
    }

    private static final class Conversation {
        final long id;
        final String title;
        final long updatedAt;
        Conversation(long id, String title, long updatedAt) {
            this.id = id;
            this.title = title == null || title.trim().isEmpty() ? "New chat" : title;
            this.updatedAt = updatedAt;
        }
    }

    private static final class Store extends android.database.sqlite.SQLiteOpenHelper {
        Store(Context c) { super(c, "saarthi_history.sqlite3", null, 2); }

        @Override public void onConfigure(android.database.sqlite.SQLiteDatabase db) {
            super.onConfigure(db);
            db.setForeignKeyConstraintsEnabled(true);
        }

        @Override public void onCreate(android.database.sqlite.SQLiteDatabase db) {
            db.execSQL("CREATE TABLE conversations (_id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL)");
            db.execSQL("CREATE TABLE messages (_id INTEGER PRIMARY KEY AUTOINCREMENT, conversation_id INTEGER NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL, meta TEXT, created_at INTEGER NOT NULL, FOREIGN KEY(conversation_id) REFERENCES conversations(_id) ON DELETE CASCADE)");
            db.execSQL("CREATE INDEX idx_messages_conversation_id ON messages(conversation_id, _id)");
            long now = System.currentTimeMillis();
            android.content.ContentValues cv = new android.content.ContentValues();
            cv.put("title", "New chat"); cv.put("created_at", now); cv.put("updated_at", now);
            db.insert("conversations", null, cv);
        }

        @Override public void onUpgrade(android.database.sqlite.SQLiteDatabase db, int oldVersion, int newVersion) {
            if (oldVersion < 2) {
                long now = System.currentTimeMillis();
                db.execSQL("CREATE TABLE conversations (_id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL)");
                db.execSQL("INSERT INTO conversations (_id, title, created_at, updated_at) VALUES (1, 'Previous conversation', " + now + ", " + now + ")");
                // Version 1 stored every message in one flat history. Assign all old rows to that conversation.
                db.execSQL("ALTER TABLE messages ADD COLUMN conversation_id INTEGER NOT NULL DEFAULT 1");
                db.execSQL("CREATE INDEX idx_messages_conversation_id ON messages(conversation_id, _id)");
                db.execSQL("UPDATE conversations SET updated_at = COALESCE((SELECT MAX(created_at) FROM messages), created_at) WHERE _id = 1");
            }
        }

        synchronized long createConversation(String title) {
            android.database.sqlite.SQLiteDatabase db = getWritableDatabase();
            long now = System.currentTimeMillis();
            android.content.ContentValues values = new android.content.ContentValues();
            values.put("title", title == null || title.trim().isEmpty() ? "New chat" : title.trim());
            values.put("created_at", now); values.put("updated_at", now);
            return db.insert("conversations", null, values);
        }

        synchronized Conversation getConversation(long id) {
            if (id <= 0) return null;
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT _id,title,updated_at FROM conversations WHERE _id=?", new String[]{String.valueOf(id)})) {
                if (c.moveToFirst()) return new Conversation(c.getLong(0), c.getString(1), c.getLong(2));
            }
            return null;
        }

        synchronized List<Conversation> allConversations() {
            List<Conversation> result = new ArrayList<>();
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT _id,title,updated_at FROM conversations ORDER BY updated_at DESC, _id DESC", null)) {
                while (c.moveToNext()) result.add(new Conversation(c.getLong(0), c.getString(1), c.getLong(2)));
            }
            return result;
        }

        synchronized void renameConversation(long id, String title) {
            android.content.ContentValues values = new android.content.ContentValues();
            values.put("title", title.trim()); values.put("updated_at", System.currentTimeMillis());
            getWritableDatabase().update("conversations", values, "_id=?", new String[]{String.valueOf(id)});
        }

        synchronized void renameIfDefaultTitle(long id, String title) {
            Conversation c = getConversation(id);
            if (c != null && "New chat".equals(c.title)) renameConversation(id, title);
        }

        synchronized void deleteConversation(long id) {
            android.database.sqlite.SQLiteDatabase db = getWritableDatabase();
            db.beginTransaction();
            try {
                // Explicit delete keeps deletion correct for rows migrated from the old schema too.
                db.delete("messages", "conversation_id=?", new String[]{String.valueOf(id)});
                db.delete("conversations", "_id=?", new String[]{String.valueOf(id)});
                db.setTransactionSuccessful();
            } finally { db.endTransaction(); }
        }

        synchronized void addMessage(long conversationId, String role, String content, String meta) {
            android.database.sqlite.SQLiteDatabase db = getWritableDatabase();
            long now = System.currentTimeMillis();
            android.content.ContentValues values = new android.content.ContentValues();
            values.put("conversation_id", conversationId); values.put("role", role); values.put("content", content);
            values.put("meta", meta); values.put("created_at", now);
            db.insert("messages", null, values);
            android.content.ContentValues updated = new android.content.ContentValues(); updated.put("updated_at", now);
            db.update("conversations", updated, "_id=?", new String[]{String.valueOf(conversationId)});
        }

        synchronized String preview(long conversationId) {
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT content FROM messages WHERE conversation_id=? ORDER BY _id DESC LIMIT 1",
                    new String[]{String.valueOf(conversationId)})) {
                if (c.moveToFirst()) {
                    String value = c.getString(0).replaceAll("\\s+", " ").trim();
                    return value.length() > 90 ? value.substring(0, 87) + "…" : value;
                }
            }
            return "";
        }

        synchronized List<Message> latestMessages(long conversationId, int limit) {
            List<Message> result = new ArrayList<>();
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT role,content,meta FROM messages WHERE conversation_id=? ORDER BY _id DESC LIMIT ?",
                    new String[]{String.valueOf(conversationId), String.valueOf(limit)})) {
                while (c.moveToNext()) result.add(new Message(c.getString(0), c.getString(1), c.getString(2)));
            }
            Collections.reverse(result); return result;
        }

        synchronized List<Message> allMessages(long conversationId, int limit) {
            List<Message> result = new ArrayList<>();
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT role,content,meta FROM messages WHERE conversation_id=? ORDER BY _id DESC LIMIT ?",
                    new String[]{String.valueOf(conversationId), String.valueOf(limit)})) {
                while (c.moveToNext()) result.add(new Message(c.getString(0), c.getString(1), c.getString(2)));
            }
            Collections.reverse(result); return result;
        }
    }

    private static final class SecureSettings {
        private final SharedPreferences prefs;
        SecureSettings(Context c) { prefs = c.getSharedPreferences("secure_config", Context.MODE_PRIVATE); }
        Config load() {
            String blob = prefs.getString("config_blob", "");
            if (blob.isEmpty()) return new Config();
            try {
                byte[] combined = Base64.decode(blob, Base64.NO_WRAP);
                byte[] iv = java.util.Arrays.copyOfRange(combined, 0, 12);
                byte[] cipherText = java.util.Arrays.copyOfRange(combined, 12, combined.length);
                Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
                cipher.init(Cipher.DECRYPT_MODE, getKey(), new GCMParameterSpec(128, iv));
                return Config.fromJson(new JSONObject(new String(cipher.doFinal(cipherText), StandardCharsets.UTF_8)));
            } catch (Exception e) { return new Config(); }
        }
        void save(Config config) throws Exception {
            Cipher cipher = Cipher.getInstance("AES/GCM/NoPadding");
            cipher.init(Cipher.ENCRYPT_MODE, getKey());
            byte[] iv = cipher.getIV(); byte[] encrypted = cipher.doFinal(config.toJson().toString().getBytes(StandardCharsets.UTF_8));
            byte[] combined = new byte[iv.length + encrypted.length]; System.arraycopy(iv, 0, combined, 0, iv.length); System.arraycopy(encrypted, 0, combined, iv.length, encrypted.length);
            prefs.edit().putString("config_blob", Base64.encodeToString(combined, Base64.NO_WRAP)).apply();
        }
        private SecretKey getKey() throws Exception {
            KeyStore ks = KeyStore.getInstance("AndroidKeyStore"); ks.load(null);
            if (ks.containsAlias(KEY_ALIAS)) return ((KeyStore.SecretKeyEntry)ks.getEntry(KEY_ALIAS, null)).getSecretKey();
            KeyGenerator generator = KeyGenerator.getInstance(KeyProperties.KEY_ALGORITHM_AES, "AndroidKeyStore");
            generator.init(new KeyGenParameterSpec.Builder(KEY_ALIAS, KeyProperties.PURPOSE_ENCRYPT | KeyProperties.PURPOSE_DECRYPT)
                    .setBlockModes(KeyProperties.BLOCK_MODE_GCM).setEncryptionPaddings(KeyProperties.ENCRYPTION_PADDING_NONE).setRandomizedEncryptionRequired(true).build());
            return generator.generateKey();
        }
    }
}
