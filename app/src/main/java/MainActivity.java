package com.saarthi.studio;

import android.app.Activity;
import android.app.AlertDialog;
import android.content.Context;
import android.content.ClipData;
import android.content.ClipboardManager;
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
import android.text.SpannableStringBuilder;
import android.text.style.BackgroundColorSpan;
import android.text.style.StyleSpan;
import android.text.style.TypefaceSpan;
import android.view.Gravity;
import android.view.View;
import android.view.ViewGroup;
import android.view.inputmethod.EditorInfo;
import android.view.inputmethod.InputMethodManager;
import android.widget.Button;
import android.widget.EditText;
import android.widget.LinearLayout;
import android.widget.HorizontalScrollView;
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
import java.util.LinkedHashSet;
import java.util.Locale;
import java.util.Date;
import java.text.SimpleDateFormat;
import java.util.regex.Matcher;
import java.util.regex.Pattern;
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
    private Button sendButton, codeButton, historyButton, newChatButton, usageButton;
    private TextView statusText, usageText, currentChatText;
    private boolean codeVisible = false;
    private boolean sending = false;
    private long currentConversationId = -1L;
    private static final String STATE_PREFS = "saarthi_chat_state";
    private static final String CURRENT_CONVERSATION_KEY = "current_conversation_id";
    private static final int REQUEST_EXPORT_MARKDOWN = 6101;
    private static final int REQUEST_EXPORT_BACKUP = 6102;
    private static final int REQUEST_IMPORT_BACKUP = 6103;
    private static final int MAX_BACKUP_BYTES = 10 * 1024 * 1024;
    private String pendingExportContent;

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

    @Override protected void onActivityResult(int requestCode, int resultCode, Intent data) {
        super.onActivityResult(requestCode, resultCode, data);
        if (requestCode == REQUEST_EXPORT_MARKDOWN || requestCode == REQUEST_EXPORT_BACKUP) {
            String output = pendingExportContent;
            pendingExportContent = null;
            if (resultCode != RESULT_OK || data == null || data.getData() == null) return;
            if (output == null) { toast("Nothing to export."); return; }
            try (OutputStream stream = getContentResolver().openOutputStream(data.getData())) {
                if (stream == null) throw new Exception("Could not open the selected file.");
                stream.write(output.getBytes(StandardCharsets.UTF_8));
                stream.flush();
                toast(requestCode == REQUEST_EXPORT_MARKDOWN ? "Conversation exported." : "Chat backup saved.");
            } catch (Exception e) {
                toast("Could not save the export: " + friendlyError(e.getMessage()));
            }
            return;
        }
        if (requestCode == REQUEST_IMPORT_BACKUP) {
            if (resultCode != RESULT_OK || data == null || data.getData() == null) return;
            importBackup(data.getData());
        }
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
        Button backupButton = smallButton("Backup");
        backupButton.setOnClickListener(v -> showBackupMenu());
        chatBar.addView(backupButton);
        newChatButton = smallButton("+ New chat");
        newChatButton.setOnClickListener(v -> createNewConversation());
        LinearLayout.LayoutParams newChatLp = new LinearLayout.LayoutParams(-2, -2);
        newChatLp.leftMargin = dp(6);
        chatBar.addView(newChatButton, newChatLp);
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
        usageButton = smallButton("Usage");
        usageButton.setOnClickListener(v -> showUsageDiagnostics());
        LinearLayout.LayoutParams usageButtonLp = new LinearLayout.LayoutParams(-2, -2);
        usageButtonLp.leftMargin = dp(6);
        toolRow.addView(usageButton, usageButtonLp);
        usageText = label("Saved locally", 11, MUTED, false);
        usageText.setGravity(Gravity.CENTER_VERTICAL | Gravity.RIGHT);
        LinearLayout.LayoutParams usageLp = new LinearLayout.LayoutParams(0, -2, 1);
        usageLp.leftMargin = dp(6);
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
        if (user) {
            TextView bubble = label(content, 15, TEXT, false);
            bubble.setTextIsSelectable(true);
            bubble.setPadding(dp(13), dp(11), dp(13), dp(11));
            bubble.setBackground(round(USER_BUBBLE, 16));
            LinearLayout.LayoutParams bp = new LinearLayout.LayoutParams(-1, -2);
            bp.leftMargin = dp(28);
            wrap.addView(bubble, bp);
        } else {
            addAssistantMarkdown(wrap, content);
        }
        if (meta != null && !meta.isEmpty()) {
            TextView m = label(meta, 10, MUTED, false);
            m.setPadding(dp(4), dp(5), dp(4), 0); wrap.addView(m);
        }
        if (scrollNow) scrollToBottom();
    }

    /** Render a small, dependency-free Markdown subset suitable for model replies. */
    private void addAssistantMarkdown(LinearLayout parent, String content) {
        String[] lines = content.split("\\n", -1);
        StringBuilder prose = new StringBuilder();
        StringBuilder code = new StringBuilder();
        boolean inCode = false;
        String language = "";
        for (String line : lines) {
            String trimmed = line.trim();
            if (trimmed.startsWith("```")) {
                if (!inCode) {
                    flushMarkdownProse(parent, prose.toString());
                    prose.setLength(0);
                    inCode = true;
                    language = trimmed.substring(3).trim();
                    code.setLength(0);
                } else {
                    addCodeBlock(parent, code.toString().replaceFirst("\\n$", ""), language);
                    code.setLength(0);
                    language = "";
                    inCode = false;
                }
            } else if (inCode) {
                code.append(line).append('\n');
            } else {
                prose.append(line).append('\n');
            }
        }
        if (inCode) addCodeBlock(parent, code.toString().replaceFirst("\\n$", ""), language);
        if (prose.length() > 0) flushMarkdownProse(parent, prose.toString());

        Button copyResponse = smallButton("Copy response");
        copyResponse.setOnClickListener(v -> copyToClipboard("Saarthi response", content));
        LinearLayout.LayoutParams copyLp = new LinearLayout.LayoutParams(-2, dp(38));
        copyLp.topMargin = dp(5);
        parent.addView(copyResponse, copyLp);
    }

    private void flushMarkdownProse(LinearLayout parent, String raw) {
        if (raw == null || raw.trim().isEmpty()) return;
        String[] lines = raw.split("\\n", -1);
        for (String original : lines) {
            String line = original.trim();
            if (line.isEmpty()) continue;
            int size = 15;
            boolean bold = false;
            int color = TEXT;
            String display = line;
            if (line.matches("^#{1,6}\\s+.*")) {
                int count = 0;
                while (count < line.length() && line.charAt(count) == '#') count++;
                display = line.substring(count).trim();
                size = Math.max(15, 21 - count);
                bold = true;
            } else if (line.matches("^[-*+]\\s+.*")) {
                display = "•  " + line.substring(2).trim();
            } else if (line.matches("^\\d+[.)]\\s+.*")) {
                display = line;
            } else if (line.startsWith("> ")) {
                display = line.substring(2).trim();
                color = MUTED;
            }
            TextView text = label("", size, color, bold);
            text.setText(formatInlineMarkdown(display));
            text.setTextIsSelectable(true);
            LinearLayout.LayoutParams lp = new LinearLayout.LayoutParams(-1, -2);
            lp.bottomMargin = dp(6);
            parent.addView(text, lp);
        }
    }

    private CharSequence formatInlineMarkdown(String source) {
        SpannableStringBuilder out = new SpannableStringBuilder();
        int i = 0;
        while (i < source.length()) {
            if (source.startsWith("**", i)) {
                int end = source.indexOf("**", i + 2);
                if (end > i + 2) {
                    int startOut = out.length();
                    out.append(source.substring(i + 2, end));
                    out.setSpan(new StyleSpan(Typeface.BOLD), startOut, out.length(), 0);
                    i = end + 2;
                    continue;
                }
            }
            if (source.charAt(i) == '`') {
                int end = source.indexOf('`', i + 1);
                if (end > i + 1) {
                    int startOut = out.length();
                    out.append(source.substring(i + 1, end));
                    out.setSpan(new TypefaceSpan("monospace"), startOut, out.length(), 0);
                    out.setSpan(new BackgroundColorSpan(PANEL_ALT), startOut, out.length(), 0);
                    i = end + 1;
                    continue;
                }
            }
            if (source.charAt(i) == '*') {
                int end = source.indexOf('*', i + 1);
                if (end > i + 1 && (i == 0 || source.charAt(i - 1) != '*')) {
                    int startOut = out.length();
                    out.append(source.substring(i + 1, end));
                    out.setSpan(new StyleSpan(Typeface.ITALIC), startOut, out.length(), 0);
                    i = end + 1;
                    continue;
                }
            }
            out.append(source.charAt(i));
            i++;
        }
        return out;
    }

    private void addCodeBlock(LinearLayout parent, String code, String language) {
        LinearLayout card = new LinearLayout(this);
        card.setOrientation(LinearLayout.VERTICAL);
        card.setPadding(dp(11), dp(8), dp(11), dp(8));
        card.setBackground(round(PANEL_ALT, 12));
        LinearLayout.LayoutParams cardLp = new LinearLayout.LayoutParams(-1, -2);
        cardLp.bottomMargin = dp(9);
        parent.addView(card, cardLp);

        LinearLayout toolbar = row();
        toolbar.setGravity(Gravity.CENTER_VERTICAL);
        String labelText = language == null || language.isEmpty() ? "CODE" : language.toUpperCase(Locale.ROOT);
        TextView languageLabel = label(labelText, 10, MUTED, true);
        toolbar.addView(languageLabel, new LinearLayout.LayoutParams(0, -2, 1));
        Button copyCode = smallButton("Copy code");
        copyCode.setOnClickListener(v -> copyToClipboard("Code", code));
        toolbar.addView(copyCode);
        card.addView(toolbar);

        HorizontalScrollView horizontal = new HorizontalScrollView(this);
        horizontal.setHorizontalScrollBarEnabled(false);
        horizontal.setFillViewport(false);
        TextView codeText = label(code, 13, TEXT, false);
        codeText.setTypeface(Typeface.MONOSPACE);
        codeText.setTextIsSelectable(true);
        codeText.setPadding(0, dp(9), dp(8), dp(7));
        horizontal.addView(codeText, new HorizontalScrollView.LayoutParams(-2, -2));
        card.addView(horizontal, new LinearLayout.LayoutParams(-1, -2));
    }

    private void copyToClipboard(String label, String text) {
        ClipboardManager clipboard = (ClipboardManager) getSystemService(CLIPBOARD_SERVICE);
        if (clipboard != null) {
            clipboard.setPrimaryClip(ClipData.newPlainText(label, text == null ? "" : text));
            toast(label + " copied to clipboard.");
        } else {
            toast("Clipboard is unavailable.");
        }
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
            int responseStatus = 0;
            String routedProvider = "";
            try {
                JSONObject body = new JSONObject(); body.put("model", model); body.put("messages", msgs); body.put("stream", false);
                HttpResult result = request("POST", endpoint(base, "chat/completions"), key, body.toString());
                responseStatus = result.status;
                routedProvider = result.headers.getOrDefault("x-routed-via", "provider not reported");
                JSONObject json = parseJson(result.body);
                if (result.status < 200 || result.status >= 300) throw new Exception(errorText(json, result.body, result.status));
                String answer = responseText(json);
                if (answer.trim().isEmpty()) throw new Exception("Router returned an empty response.");
                JSONObject usage = json.optJSONObject("usage");
                int reportedTokens = -1;
                if (usage != null && usage.has("total_tokens")) {
                    Object tokenValue = usage.opt("total_tokens");
                    try { reportedTokens = tokenValue instanceof Number ? ((Number) tokenValue).intValue() : Integer.parseInt(String.valueOf(tokenValue)); }
                    catch (Exception ignored) { reportedTokens = -1; }
                }
                String tokenText = reportedTokens < 0 ? "token count not reported" : reportedTokens + " tokens";
                long ms = System.currentTimeMillis() - started;
                String meta = tokenText + " · " + String.format(Locale.US, "%.1f s", ms / 1000.0) + " · " + routedProvider;
                store.addRequestLog(model, true, responseStatus, reportedTokens, ms, routedProvider, "");
                store.addMessage(targetConversationId, "assistant", answer, meta);
                main.post(() -> {
                    setSending(false); renderHistory(); statusText.setText("Connected · " + model); usageText.setText(meta); codeInput.setText("");
                });
            } catch (Exception e) {
                String message = friendlyError(e.getMessage());
                long ms = System.currentTimeMillis() - started;
                String meta = "Failed after " + String.format(Locale.US, "%.1f s", ms / 1000.0);
                store.addRequestLog(model, false, responseStatus, -1, ms, routedProvider, safeDiagnostic(message, key));
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

    private void showBackupMenu() {
        String[] options = new String[]{
                "Export current conversation (.md)",
                "Export all chats (backup .json)",
                "Restore chats from backup (.json)"
        };
        new AlertDialog.Builder(this)
                .setTitle("Backup & export")
                .setItems(options, (dialog, which) -> {
                    if (which == 0) exportCurrentConversation();
                    else if (which == 1) exportAllConversations();
                    else if (which == 2) confirmRestoreBackup();
                })
                .show();
    }

    private void exportCurrentConversation() {
        Conversation conversation = store.getConversation(currentConversationId);
        if (conversation == null) { toast("There is no current conversation to export."); return; }
        List<Message> messages = store.allMessages(currentConversationId, 10000);
        StringBuilder markdown = new StringBuilder();
        markdown.append("# ").append(conversation.title.replace("\n", " ").replace("\r", " "))
                .append("\n\nExported from Saarthi Studio on ")
                .append(new SimpleDateFormat("yyyy-MM-dd HH:mm", Locale.getDefault()).format(new Date()))
                .append(".\n\n");
        for (Message message : messages) {
            String heading = "user".equals(message.role) ? "You" : ("assistant".equals(message.role) ? "Saarthi" : message.role);
            markdown.append("## ").append(heading).append("\n\n")
                    .append(message.content == null ? "" : message.content).append("\n\n");
            if (message.meta != null && !message.meta.trim().isEmpty()) {
                markdown.append("_ ").append(message.meta).append(" _\n\n");
            }
            markdown.append("---\n\n");
        }
        startSaveDocument(markdown.toString(), "text/markdown", safeFilename(conversation.title, ".md"), REQUEST_EXPORT_MARKDOWN);
    }

    private void exportAllConversations() {
        try {
            JSONObject backup = new JSONObject();
            backup.put("format", "saarthi-studio-backup");
            backup.put("schema_version", 1);
            backup.put("exported_at", System.currentTimeMillis());
            JSONArray conversationsJson = new JSONArray();
            List<Conversation> all = store.allConversations();
            if (all.size() > 500) throw new Exception("Too many conversations for one backup (maximum 500).");
            int totalMessages = 0;
            for (Conversation conversation : all) {
                JSONObject item = new JSONObject();
                item.put("title", conversation.title);
                item.put("updated_at", conversation.updatedAt);
                JSONArray messagesJson = new JSONArray();
                List<Message> messages = store.allMessages(conversation.id, 10000);
                totalMessages += messages.size();
                if (totalMessages > 20000) throw new Exception("Too many messages for one backup (maximum 20,000).");
                for (Message message : messages) {
                    JSONObject messageJson = new JSONObject();
                    messageJson.put("role", message.role);
                    messageJson.put("content", message.content);
                    messageJson.put("meta", message.meta == null ? "" : message.meta);
                    messagesJson.put(messageJson);
                }
                item.put("messages", messagesJson);
                conversationsJson.put(item);
            }
            backup.put("conversations", conversationsJson);
            String filename = "saarthi-studio-backup-" + new SimpleDateFormat("yyyyMMdd-HHmm", Locale.US).format(new Date()) + ".json";
            String jsonText = backup.toString(2);
            if (jsonText.getBytes(StandardCharsets.UTF_8).length > MAX_BACKUP_BYTES) {
                throw new Exception("Backup would exceed 10 MB. Export individual conversations instead.");
            }
            startSaveDocument(jsonText, "application/json", filename, REQUEST_EXPORT_BACKUP);
        } catch (Exception e) {
            toast("Could not prepare the backup: " + friendlyError(e.getMessage()));
        }
    }

    private void confirmRestoreBackup() {
        new AlertDialog.Builder(this)
                .setTitle("Restore chat backup?")
                .setMessage("Choose a Saarthi Studio JSON backup. Imported conversations will be added to this phone; your existing chats will not be deleted. Importing the same file twice can create duplicates.")
                .setNegativeButton("Cancel", null)
                .setPositiveButton("Choose backup", (dialog, which) -> {
                    Intent intent = new Intent(Intent.ACTION_OPEN_DOCUMENT);
                    intent.addCategory(Intent.CATEGORY_OPENABLE);
                    intent.setType("application/json");
                    try { startActivityForResult(intent, REQUEST_IMPORT_BACKUP); }
                    catch (Exception e) { toast("Could not open the file picker."); }
                })
                .show();
    }

    private void startSaveDocument(String content, String mimeType, String filename, int requestCode) {
        pendingExportContent = content;
        Intent intent = new Intent(Intent.ACTION_CREATE_DOCUMENT);
        intent.addCategory(Intent.CATEGORY_OPENABLE);
        intent.setType(mimeType);
        intent.putExtra(Intent.EXTRA_TITLE, filename);
        try {
            startActivityForResult(intent, requestCode);
        } catch (Exception e) {
            pendingExportContent = null;
            toast("Could not open the save dialog.");
        }
    }

    private static String safeFilename(String title, String extension) {
        String safe = title == null ? "conversation" : title.trim();
        safe = safe.replaceAll("[^A-Za-z0-9._-]+", "-").replaceAll("-+", "-");
        safe = safe.replaceAll("^[.-]+|[.-]+$", "");
        if (safe.isEmpty()) safe = "conversation";
        if (safe.length() > 70) safe = safe.substring(0, 70);
        return safe + extension;
    }

    private void importBackup(Uri uri) {
        executor.execute(() -> {
            try (InputStream stream = getContentResolver().openInputStream(uri)) {
                if (stream == null) throw new Exception("Could not open the selected backup file.");
                byte[] bytes = readLimited(stream, MAX_BACKUP_BYTES);
                JSONObject backup = new JSONObject(new String(bytes, StandardCharsets.UTF_8));
                if (!"saarthi-studio-backup".equals(backup.optString("format"))) {
                    throw new Exception("This file is not a Saarthi Studio backup.");
                }
                if (backup.optInt("schema_version", -1) != 1) {
                    throw new Exception("This backup version is not supported by this app.");
                }
                JSONArray inputConversations = backup.optJSONArray("conversations");
                if (inputConversations == null || inputConversations.length() == 0) {
                    throw new Exception("The backup does not contain any conversations.");
                }
                if (inputConversations.length() > 500) throw new Exception("The backup contains too many conversations (maximum 500).");

                List<ImportedConversation> parsed = new ArrayList<>();
                int totalMessages = 0;
                for (int i = 0; i < inputConversations.length(); i++) {
                    JSONObject item = inputConversations.optJSONObject(i);
                    if (item == null) throw new Exception("Conversation " + (i + 1) + " is invalid.");
                    String title = item.optString("title", "Imported chat").trim();
                    if (title.isEmpty()) title = "Imported chat";
                    if (title.length() > 160) title = title.substring(0, 160);
                    JSONArray inputMessages = item.optJSONArray("messages");
                    if (inputMessages == null) inputMessages = new JSONArray();
                    if (inputMessages.length() > 10000) throw new Exception("A conversation contains too many messages.");
                    List<Message> messages = new ArrayList<>();
                    for (int j = 0; j < inputMessages.length(); j++) {
                        JSONObject message = inputMessages.optJSONObject(j);
                        if (message == null) continue;
                        String role = message.optString("role", "");
                        if (!"user".equals(role) && !"assistant".equals(role)) continue;
                        String content = message.optString("content", "");
                        if (content.length() > 500000) throw new Exception("A message is too large to import.");
                        String meta = message.optString("meta", "");
                        if (meta.length() > 1000) meta = meta.substring(0, 1000);
                        messages.add(new Message(role, content, meta));
                        totalMessages++;
                        if (totalMessages > 20000) throw new Exception("The backup contains too many messages (maximum 20,000).");
                    }
                    parsed.add(new ImportedConversation(title, messages));
                }
                if (parsed.isEmpty()) throw new Exception("No valid conversations were found in this backup.");
                List<Long> importedIds = store.importConversations(parsed);
                long currentId = importedIds.get(importedIds.size() - 1);
                int importedMessageCount = totalMessages;
                main.post(() -> {
                    currentConversationId = currentId;
                    saveCurrentConversationId();
                    renderConversationTitle();
                    renderHistory();
                    usageText.setText("Backup restored · " + importedIds.size() + " conversations");
                    toast("Restored " + importedIds.size() + " conversations and " + importedMessageCount + " messages. Existing chats were kept.");
                });
            } catch (Exception e) {
                String message = e.getMessage() == null ? "Unknown import error." : e.getMessage();
                main.post(() -> toast("Could not restore backup: " + friendlyError(message)));
            }
        });
    }

    private static byte[] readLimited(InputStream input, int maxBytes) throws Exception {
        try (InputStream in = input; ByteArrayOutputStream out = new ByteArrayOutputStream()) {
            byte[] buffer = new byte[8192];
            int count;
            int total = 0;
            while ((count = in.read(buffer)) != -1) {
                total += count;
                if (total > maxBytes) throw new Exception("Backup file is too large (maximum 10 MB).");
                out.write(buffer, 0, count);
            }
            return out.toByteArray();
        }
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
        EditText model = field("Model ID (default: auto)", config.model, InputType.TYPE_CLASS_TEXT);
        form.addView(model, fieldLp());
        TextView modelHelp = label("Use auto for router-managed selection, load model IDs from FreeLLMAPI, or type a model ID manually.", 12, MUTED, false);
        modelHelp.setPadding(dp(2), 0, dp(2), dp(9));
        form.addView(modelHelp);
        EditText key = field("Unified API key (leave blank to keep saved key)", "", InputType.TYPE_CLASS_TEXT | InputType.TYPE_TEXT_VARIATION_PASSWORD);
        form.addView(key, fieldLp());
        Button loadModels = smallButton("Load available models");
        loadModels.setTextColor(GREEN);
        LinearLayout.LayoutParams loadModelsLp = new LinearLayout.LayoutParams(-1, dp(42));
        loadModelsLp.bottomMargin = dp(10);
        form.addView(loadModels, loadModelsLp);
        loadModels.setOnClickListener(v -> fetchModels(base, key, model));
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

    private void fetchModels(EditText baseField, EditText keyField, EditText modelField) {
        String url = baseField.getText().toString().trim().replaceAll("/+$", "");
        String key = keyField.getText().toString().trim().isEmpty()
                ? config.apiKey : keyField.getText().toString().trim();
        if (!validUrl(url)) {
            baseField.setError("Use a valid http:// or https:// URL ending in /v1");
            return;
        }
        if (key.isEmpty()) {
            keyField.setError("Enter your unified API key, or save one in Settings first");
            return;
        }
        toast("Loading model list…");
        executor.execute(() -> {
            try {
                HttpResult response = request("GET", endpoint(url, "models"), key, null);
                JSONObject json = parseJson(response.body);
                if (response.status < 200 || response.status >= 300) {
                    throw new Exception(errorText(json, response.body, response.status));
                }
                JSONArray data = json.optJSONArray("data");
                if (data == null) data = json.optJSONArray("models");
                LinkedHashSet<String> ids = new LinkedHashSet<>();
                ids.add("auto");
                if (data != null) {
                    for (int i = 0; i < data.length(); i++) {
                        Object item = data.opt(i);
                        String id = "";
                        if (item instanceof JSONObject) {
                            JSONObject obj = (JSONObject) item;
                            id = obj.optString("id", obj.optString("name", ""));
                        } else if (item instanceof String) {
                            id = (String) item;
                        }
                        id = id == null ? "" : id.trim();
                        if (!id.isEmpty()) ids.add(id);
                    }
                }
                if (ids.size() <= 1) {
                    main.post(() -> toast("FreeLLMAPI did not return model IDs. Keep using auto or type a model ID manually."));
                    return;
                }
                List<String> models = new ArrayList<>(ids);
                main.post(() -> showModelPicker(modelField, models));
            } catch (Exception e) {
                String message = friendlyError(e.getMessage());
                main.post(() -> toast("Could not load models: " + message));
            }
        });
    }

    private void showModelPicker(EditText modelField, List<String> models) {
        String current = modelField.getText().toString().trim();
        int selected = models.indexOf(current);
        new AlertDialog.Builder(this)
                .setTitle("Choose a model")
                .setSingleChoiceItems(models.toArray(new String[0]), selected, (dialog, which) -> {
                    modelField.setText(models.get(which));
                    dialog.dismiss();
                })
                .setNegativeButton("Cancel", null)
                .show();
    }

    private void showUsageDiagnostics() {
        UsageSummary stats = store.getUsageSummary();
        List<RequestLog> recent = store.recentRequestLogs(12);

        LinearLayout content = new LinearLayout(this);
        content.setOrientation(LinearLayout.VERTICAL);
        content.setPadding(dp(16), dp(8), dp(16), dp(8));

        TextView intro = label("Local request records from this phone. Token totals include only values reported by FreeLLMAPI; this screen cannot see provider-side quota balances.", 12, MUTED, false);
        intro.setPadding(0, 0, 0, dp(12));
        content.addView(intro);

        addDiagnosticStat(content, "AI requests", String.valueOf(stats.totalRequests));
        addDiagnosticStat(content, "Successful", String.valueOf(stats.successfulRequests));
        addDiagnosticStat(content, "Failed", String.valueOf(stats.failedRequests));
        addDiagnosticStat(content, "Reported tokens", formatInteger(stats.reportedTokens));
        addDiagnosticStat(content, "Average response time", stats.timedRequests == 0 ? "—" : String.format(Locale.getDefault(), "%.2f s", stats.averageDurationMs / 1000.0));

        TextView recentTitle = label("Recent requests", 15, TEXT, true);
        recentTitle.setPadding(0, dp(17), 0, dp(7));
        content.addView(recentTitle);
        if (recent.isEmpty()) {
            TextView empty = label("No requests recorded yet. Send a message to start tracking usage.", 13, MUTED, false);
            empty.setPadding(0, dp(4), 0, dp(10));
            content.addView(empty);
        } else {
            SimpleDateFormat dateFormat = new SimpleDateFormat("dd MMM, HH:mm", Locale.getDefault());
            for (RequestLog entry : recent) {
                LinearLayout card = new LinearLayout(this);
                card.setOrientation(LinearLayout.VERTICAL);
                card.setPadding(dp(11), dp(9), dp(11), dp(9));
                card.setBackground(round(PANEL, 12));
                LinearLayout.LayoutParams cardLp = new LinearLayout.LayoutParams(-1, -2);
                cardLp.bottomMargin = dp(7);
                content.addView(card, cardLp);

                String state = entry.success ? "✓ Success" : "✕ Failed";
                TextView title = label(state + " · " + entry.model, 13, entry.success ? GREEN : Color.rgb(255, 137, 137), true);
                card.addView(title);
                StringBuilder detail = new StringBuilder(dateFormat.format(new Date(entry.createdAt)));
                if (entry.durationMs >= 0) detail.append(" · ").append(String.format(Locale.US, "%.2f s", entry.durationMs / 1000.0));
                if (entry.tokens >= 0) detail.append(" · ").append(entry.tokens).append(" tokens");
                if (entry.statusCode > 0) detail.append(" · HTTP ").append(entry.statusCode);
                if (entry.provider != null && !entry.provider.trim().isEmpty()) detail.append(" · ").append(entry.provider);
                TextView meta = label(detail.toString(), 11, MUTED, false);
                meta.setPadding(0, dp(4), 0, 0);
                card.addView(meta);
                if (!entry.success && entry.detail != null && !entry.detail.trim().isEmpty()) {
                    TextView error = label(entry.detail, 12, TEXT, false);
                    error.setPadding(0, dp(5), 0, 0);
                    card.addView(error);
                }
            }
        }

        ScrollView scroller = new ScrollView(this);
        scroller.setFillViewport(false);
        scroller.addView(content);
        AlertDialog dialog = new AlertDialog.Builder(this)
                .setTitle("Usage & Diagnostics")
                .setView(scroller)
                .setNegativeButton("Close", (d, which) -> d.dismiss())
                .setNeutralButton("Test connection", null)
                .create();
        dialog.setOnShowListener(ignored -> dialog.getButton(AlertDialog.BUTTON_NEUTRAL).setOnClickListener(v -> {
            dialog.dismiss();
            testConfiguredConnection();
        }));
        dialog.show();
        dialog.getWindow().setLayout(Math.min(getResources().getDisplayMetrics().widthPixels - dp(28), dp(520)), dp(600));
    }

    private void addDiagnosticStat(LinearLayout parent, String name, String value) {
        LinearLayout line = row();
        line.setGravity(Gravity.CENTER_VERTICAL);
        line.setPadding(dp(1), dp(7), dp(1), dp(7));
        TextView key = label(name, 13, MUTED, false);
        line.addView(key, new LinearLayout.LayoutParams(0, -2, 1));
        TextView val = label(value, 14, TEXT, true);
        line.addView(val);
        parent.addView(line);
        View separator = new View(this);
        separator.setBackgroundColor(Color.rgb(44, 51, 65));
        parent.addView(separator, new LinearLayout.LayoutParams(-1, dp(1)));
    }

    private void testConfiguredConnection() {
        if (config.baseUrl == null || config.baseUrl.trim().isEmpty() || config.apiKey == null || config.apiKey.trim().isEmpty()) {
            toast("Open Settings and save the FreeLLMAPI URL and unified API key first.");
            openSettings();
            return;
        }
        statusText.setText("Testing FreeLLMAPI connection…");
        executor.execute(() -> {
            String message;
            boolean ok = false;
            try {
                HttpResult result = request("GET", endpoint(config.baseUrl, "models"), config.apiKey, null);
                ok = result.status >= 200 && result.status < 300;
                message = ok ? "Connection successful. FreeLLMAPI is reachable." : "Server responded with HTTP " + result.status + ". Check the URL, key, and server logs.";
            } catch (Exception e) {
                message = "Could not reach FreeLLMAPI: " + friendlyError(e.getMessage());
            }
            final boolean success = ok;
            final String finalMessage = message;
            main.post(() -> {
                statusText.setText(success ? "FreeLLMAPI connection successful" : "Connection test failed");
                toast(finalMessage);
            });
        });
    }

    private static String safeDiagnostic(String message, String secret) {
        String safe = message == null ? "Unknown error" : message.replaceAll("[\\r\\n\\t]+", " ").trim();
        if (secret != null && secret.length() >= 4) safe = safe.replace(secret, "[redacted]");
        return safe.length() > 260 ? safe.substring(0, 257) + "…" : safe;
    }

    private static String formatInteger(long value) {
        return String.format(Locale.getDefault(), "%,d", value);
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
    private static final class ImportedConversation {
        final String title;
        final List<Message> messages;
        ImportedConversation(String title, List<Message> messages) {
            this.title = title == null || title.trim().isEmpty() ? "Imported chat" : title.trim();
            this.messages = messages == null ? new ArrayList<>() : messages;
        }
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

    private static final class UsageSummary {
        long totalRequests;
        long successfulRequests;
        long failedRequests;
        long reportedTokens;
        double averageDurationMs;
        long timedRequests;
    }

    private static final class RequestLog {
        final String model;
        final boolean success;
        final int statusCode;
        final int tokens;
        final long durationMs;
        final String provider;
        final String detail;
        final long createdAt;
        RequestLog(String model, boolean success, int statusCode, int tokens, long durationMs, String provider, String detail, long createdAt) {
            this.model = model == null ? "auto" : model;
            this.success = success;
            this.statusCode = statusCode;
            this.tokens = tokens;
            this.durationMs = durationMs;
            this.provider = provider == null ? "" : provider;
            this.detail = detail == null ? "" : detail;
            this.createdAt = createdAt;
        }
    }

    private static final class Store extends android.database.sqlite.SQLiteOpenHelper {
        Store(Context c) { super(c, "saarthi_history.sqlite3", null, 3); }

        @Override public void onConfigure(android.database.sqlite.SQLiteDatabase db) {
            super.onConfigure(db);
            db.setForeignKeyConstraintsEnabled(true);
        }

        @Override public void onCreate(android.database.sqlite.SQLiteDatabase db) {
            db.execSQL("CREATE TABLE conversations (_id INTEGER PRIMARY KEY AUTOINCREMENT, title TEXT NOT NULL, created_at INTEGER NOT NULL, updated_at INTEGER NOT NULL)");
            db.execSQL("CREATE TABLE messages (_id INTEGER PRIMARY KEY AUTOINCREMENT, conversation_id INTEGER NOT NULL, role TEXT NOT NULL, content TEXT NOT NULL, meta TEXT, created_at INTEGER NOT NULL, FOREIGN KEY(conversation_id) REFERENCES conversations(_id) ON DELETE CASCADE)");
            db.execSQL("CREATE INDEX idx_messages_conversation_id ON messages(conversation_id, _id)");
            createRequestLogTable(db);
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
            if (oldVersion < 3) {
                createRequestLogTable(db);
                // Backfill diagnostics from saved assistant replies so upgrading does not start with an empty dashboard.
                try (android.database.Cursor c = db.rawQuery("SELECT meta,content,created_at FROM messages WHERE role='assistant' ORDER BY _id", null)) {
                    while (c.moveToNext()) {
                        String meta = c.getString(0) == null ? "" : c.getString(0);
                        String content = c.getString(1) == null ? "" : c.getString(1);
                        boolean success = !meta.startsWith("Failed after") && !content.startsWith("FreeLLMAPI returned HTTP ");
                        int tokens = parseTokensFromMeta(meta);
                        long duration = parseDurationFromMeta(meta);
                        String provider = parseProviderFromMeta(meta);
                        int statusCode = parseStatusCode(content);
                        android.content.ContentValues values = new android.content.ContentValues();
                        values.put("model", "historical");
                        values.put("success", success ? 1 : 0);
                        values.put("status_code", statusCode);
                        values.put("tokens", tokens);
                        values.put("duration_ms", duration);
                        values.put("provider", provider);
                        values.put("detail", success ? "" : truncateDiagnostic(content));
                        values.put("created_at", c.getLong(2));
                        db.insert("request_logs", null, values);
                    }
                }
            }
        }

        private static void createRequestLogTable(android.database.sqlite.SQLiteDatabase db) {
            db.execSQL("CREATE TABLE IF NOT EXISTS request_logs (_id INTEGER PRIMARY KEY AUTOINCREMENT, model TEXT NOT NULL, success INTEGER NOT NULL, status_code INTEGER NOT NULL DEFAULT 0, tokens INTEGER NOT NULL DEFAULT -1, duration_ms INTEGER NOT NULL DEFAULT -1, provider TEXT NOT NULL DEFAULT '', detail TEXT NOT NULL DEFAULT '', created_at INTEGER NOT NULL)");
            db.execSQL("CREATE INDEX IF NOT EXISTS idx_request_logs_created_at ON request_logs(created_at DESC, _id DESC)");
        }

        private static int parseTokensFromMeta(String meta) {
            Matcher m = Pattern.compile("(\\d+)\\s+tokens").matcher(meta == null ? "" : meta);
            if (m.find()) { try { return Integer.parseInt(m.group(1)); } catch (Exception ignored) { } }
            return -1;
        }

        private static long parseDurationFromMeta(String meta) {
            Matcher m = Pattern.compile("([0-9]+(?:\\.[0-9]+)?)\\s*s").matcher(meta == null ? "" : meta);
            if (m.find()) { try { return (long)(Double.parseDouble(m.group(1)) * 1000.0); } catch (Exception ignored) { } }
            return -1;
        }

        private static String parseProviderFromMeta(String meta) {
            if (meta == null || meta.trim().isEmpty() || meta.startsWith("Failed after")) return "";
            String[] parts = meta.split(" · ", 3);
            return parts.length >= 3 ? parts[2] : "";
        }

        private static int parseStatusCode(String text) {
            Matcher m = Pattern.compile("HTTP\\s+(\\d{3})").matcher(text == null ? "" : text);
            if (m.find()) { try { return Integer.parseInt(m.group(1)); } catch (Exception ignored) { } }
            return 0;
        }

        private static String truncateDiagnostic(String text) {
            String value = text == null ? "" : text.replaceAll("[\\r\\n\\t]+", " ").trim();
            return value.length() > 260 ? value.substring(0, 257) + "…" : value;
        }

        synchronized void addRequestLog(String model, boolean success, int statusCode, int tokens, long durationMs, String provider, String detail) {
            android.content.ContentValues values = new android.content.ContentValues();
            values.put("model", model == null || model.trim().isEmpty() ? "auto" : model.trim());
            values.put("success", success ? 1 : 0);
            values.put("status_code", statusCode);
            values.put("tokens", tokens);
            values.put("duration_ms", durationMs);
            values.put("provider", provider == null ? "" : provider);
            values.put("detail", detail == null ? "" : detail);
            values.put("created_at", System.currentTimeMillis());
            getWritableDatabase().insert("request_logs", null, values);
        }

        synchronized UsageSummary getUsageSummary() {
            UsageSummary summary = new UsageSummary();
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT COUNT(*), COALESCE(SUM(success),0), COALESCE(SUM(CASE WHEN success=0 THEN 1 ELSE 0 END),0), COALESCE(SUM(CASE WHEN tokens>=0 THEN tokens ELSE 0 END),0), COALESCE(AVG(CASE WHEN duration_ms>=0 THEN duration_ms END),0), COALESCE(SUM(CASE WHEN duration_ms>=0 THEN 1 ELSE 0 END),0) FROM request_logs", null)) {
                if (c.moveToFirst()) {
                    summary.totalRequests = c.getLong(0);
                    summary.successfulRequests = c.getLong(1);
                    summary.failedRequests = c.getLong(2);
                    summary.reportedTokens = c.getLong(3);
                    summary.averageDurationMs = c.getDouble(4);
                    summary.timedRequests = c.getLong(5);
                }
            }
            return summary;
        }

        synchronized List<RequestLog> recentRequestLogs(int limit) {
            List<RequestLog> result = new ArrayList<>();
            try (android.database.Cursor c = getReadableDatabase().rawQuery(
                    "SELECT model,success,status_code,tokens,duration_ms,provider,detail,created_at FROM request_logs ORDER BY created_at DESC, _id DESC LIMIT ?",
                    new String[]{String.valueOf(limit)})) {
                while (c.moveToNext()) result.add(new RequestLog(c.getString(0), c.getInt(1) != 0, c.getInt(2), c.getInt(3), c.getLong(4), c.getString(5), c.getString(6), c.getLong(7)));
            }
            return result;
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

        synchronized List<Long> importConversations(List<ImportedConversation> imported) {
            List<Long> ids = new ArrayList<>();
            android.database.sqlite.SQLiteDatabase db = getWritableDatabase();
            db.beginTransaction();
            try {
                for (ImportedConversation conversation : imported) {
                    long now = System.currentTimeMillis();
                    android.content.ContentValues conversationValues = new android.content.ContentValues();
                    conversationValues.put("title", conversation.title);
                    conversationValues.put("created_at", now);
                    conversationValues.put("updated_at", now);
                    long id = db.insert("conversations", null, conversationValues);
                    if (id < 0) throw new IllegalStateException("Could not create an imported conversation.");
                    ids.add(id);
                    for (Message message : conversation.messages) {
                        android.content.ContentValues messageValues = new android.content.ContentValues();
                        messageValues.put("conversation_id", id);
                        messageValues.put("role", message.role);
                        messageValues.put("content", message.content);
                        messageValues.put("meta", message.meta == null ? "" : message.meta);
                        messageValues.put("created_at", now);
                        if (db.insert("messages", null, messageValues) < 0) {
                            throw new IllegalStateException("Could not import a message.");
                        }
                    }
                }
                db.setTransactionSuccessful();
            } finally {
                db.endTransaction();
            }
            return ids;
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
