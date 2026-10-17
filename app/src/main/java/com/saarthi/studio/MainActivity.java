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
    private Button sendButton, codeButton;
    private TextView statusText, usageText, welcomeText;
    private boolean codeVisible = false;
    private boolean sending = false;

    @Override public void onCreate(Bundle state) {
        super.onCreate(state);
        getWindow().setStatusBarColor(BG);
        getWindow().setNavigationBarColor(BG);
        store = new Store(this);
        secureSettings = new SecureSettings(this);
        config = secureSettings.load();
        buildUi();
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
        Button settings = smallButton("Settings");
        settings.setOnClickListener(v -> openSettings());
        header.addView(settings);
        root.addView(header);

        statusText = label("Not connected", 12, MUTED, false);
        statusText.setPadding(dp(4), dp(9), dp(4), dp(9));
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
        List<Message> history = store.allMessages(150);
        if (history.isEmpty()) {
            LinearLayout welcome = new LinearLayout(this);
            welcome.setOrientation(LinearLayout.VERTICAL);
            welcome.setPadding(dp(18), dp(18), dp(18), dp(18));
            welcome.setBackground(round(PANEL, 18));
            TextView title = label("Welcome to Saarthi Studio", 20, TEXT, true);
            welcome.addView(title);
            TextView body = label("Connect your FreeLLMAPI server in Settings, then ask questions, debug code, or explore ideas. Your chat history stays on this phone.", 14, MUTED, false);
            body.setPadding(0, dp(9), 0, 0); welcome.addView(body);
            TextView steps = label("Quick start\n1. Run FreeLLMAPI on your Ubuntu computer or server.\n2. Enter its network URL and unified API key in Settings.\n3. Tap Test connection, then start chatting.", 13, TEXT, false);
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
        store.addMessage("user", fullText, "");
        messageList.removeAllViews();
        for (Message m : store.allMessages(150)) addBubble(m.role, m.content, m.meta, false);
        promptInput.setText("");
        ((InputMethodManager)getSystemService(INPUT_METHOD_SERVICE)).hideSoftInputFromWindow(promptInput.getWindowToken(), 0);
        setSending(true);
        statusText.setText("Waiting for provider response…");
        usageText.setText("Sending latest 4 messages");

        List<Message> context = store.latestMessages(4);
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
                store.addMessage("assistant", answer, meta);
                main.post(() -> {
                    setSending(false); renderHistory(); statusText.setText("Connected · " + model); usageText.setText(meta); codeInput.setText("");
                });
            } catch (Exception e) {
                String message = friendlyError(e.getMessage());
                long ms = System.currentTimeMillis() - started;
                String meta = "Failed after " + String.format(Locale.US, "%.1f s", ms / 1000.0);
                store.addMessage("assistant", message, meta);
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

    private static final class Store extends android.database.sqlite.SQLiteOpenHelper {
        Store(Context c) { super(c, "saarthi_history.sqlite3", null, 1); }
        @Override public void onCreate(android.database.sqlite.SQLiteDatabase db) {
            db.execSQL("CREATE TABLE messages (_id INTEGER PRIMARY KEY AUTOINCREMENT, role TEXT NOT NULL, content TEXT NOT NULL, meta TEXT, created_at INTEGER NOT NULL)");
        }
        @Override public void onUpgrade(android.database.sqlite.SQLiteDatabase db, int oldVersion, int newVersion) { }
        synchronized void addMessage(String role, String content, String meta) {
            android.content.ContentValues values = new android.content.ContentValues(); values.put("role", role); values.put("content", content); values.put("meta", meta); values.put("created_at", System.currentTimeMillis());
            getWritableDatabase().insert("messages", null, values);
        }
        synchronized List<Message> latestMessages(int limit) {
            List<Message> result = new ArrayList<>();
            try (android.database.Cursor c = getReadableDatabase().rawQuery("SELECT role,content,meta FROM messages ORDER BY _id DESC LIMIT ?", new String[]{String.valueOf(limit)})) {
                while (c.moveToNext()) result.add(new Message(c.getString(0), c.getString(1), c.getString(2)));
            }
            Collections.reverse(result); return result;
        }
        synchronized List<Message> allMessages(int limit) {
            List<Message> result = new ArrayList<>();
            try (android.database.Cursor c = getReadableDatabase().rawQuery("SELECT role,content,meta FROM messages ORDER BY _id DESC LIMIT ?", new String[]{String.valueOf(limit)})) {
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
