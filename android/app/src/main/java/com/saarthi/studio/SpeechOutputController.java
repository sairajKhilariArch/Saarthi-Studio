package com.saarthi.studio;

import android.content.Context;
import android.speech.tts.TextToSpeech;

/** Owns TextToSpeech lifecycle independently of the Activity UI. */
final class SpeechOutputController {
    private final TextToSpeech engine;
    private volatile boolean ready;

    SpeechOutputController(Context context) {
        engine = new TextToSpeech(context.getApplicationContext(), status -> ready = status == TextToSpeech.SUCCESS);
    }
    boolean isReady() { return ready; }
    boolean speak(String text) {
        if (engine == null || !ready || text == null || text.trim().isEmpty()) return false;
        String value = text.trim();
        if (value.length() > 3500) value = value.substring(0, 3500) + "… Text shortened for speech.";
        return engine.speak(value, TextToSpeech.QUEUE_FLUSH, null, "saarthi-response") != TextToSpeech.ERROR;
    }
    boolean stopIfSpeaking() {
        if (engine == null || !engine.isSpeaking()) return false;
        engine.stop();
        return true;
    }
    void shutdown() {
        if (engine != null) { engine.stop(); engine.shutdown(); }
        ready = false;
    }
}
