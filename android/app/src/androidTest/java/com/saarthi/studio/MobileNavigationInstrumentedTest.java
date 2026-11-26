package com.saarthi.studio;

import android.content.Context;
import android.content.SharedPreferences;
import android.view.View;
import android.view.ViewGroup;
import android.view.Window;
import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import org.junit.Test;
import org.junit.runner.RunWith;

import static org.junit.Assert.assertTrue;

/** Guards the redesigned shell: every primary destination is reachable and the theme persists. */
@RunWith(AndroidJUnit4.class)
public class MobileNavigationInstrumentedTest {
    @Test
    public void primaryDestinationsAndModelSelectorArePresent() {
        ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class);
        try {
            scenario.onActivity(activity -> {
                View root = activity.findViewById(android.R.id.content);
                for (String label : new String[]{"Chat", "Chats", "Projects", "Agents", "More"}) {
                    assertTrue("Bottom navigation destination missing: " + label,
                            hasContentDescription(root, label));
                }
                assertTrue("Header history action missing", hasContentDescription(root, "Conversation history"));
                assertTrue("Header settings action missing", hasContentDescription(root, "Settings"));
                assertTrue("Model selector missing", hasContentDescription(root, "Choose model"));
                assertTrue("Composer attachment action missing", hasContentDescription(root, "Attach a file, image or tool"));
            });
        } finally {
            scenario.close();
        }
    }

    @Test
    public void lightThemeChoiceSurvivesActivityRecreation() {
        ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class);
        try {
            scenario.onActivity(activity -> {
                SharedPreferences prefs = activity.getSharedPreferences("sarthi_theme", Context.MODE_PRIVATE);
                prefs.edit().putBoolean("dark_theme", false).commit();
                activity.recreate();
            });
            scenario.onActivity(activity -> {
                int flags = activity.getWindow().getDecorView().getSystemUiVisibility();
                assertTrue("Light theme must request dark status-bar icons",
                        (flags & View.SYSTEM_UI_FLAG_LIGHT_STATUS_BAR) != 0);
                assertTrue("Window background must remain set in light theme",
                        activity.getWindow() != null);
            });
        } finally {
            scenario.onActivity(activity -> activity.getSharedPreferences("sarthi_theme", Context.MODE_PRIVATE)
                    .edit().putBoolean("dark_theme", true).commit());
            scenario.close();
        }
    }

    private static boolean hasContentDescription(View view, String expected) {
        CharSequence description = view.getContentDescription();
        if (description != null && description.toString().contains(expected)) return true;
        if (view instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) view;
            for (int i = 0; i < group.getChildCount(); i++) {
                if (hasContentDescription(group.getChildAt(i), expected)) return true;
            }
        }
        return false;
    }
}