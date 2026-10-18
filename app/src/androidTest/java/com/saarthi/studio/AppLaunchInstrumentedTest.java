package com.saarthi.studio;

import android.view.View;
import android.view.ViewGroup;
import android.widget.EditText;
import android.widget.TextView;
import androidx.test.core.app.ActivityScenario;
import androidx.test.ext.junit.runners.AndroidJUnit4;
import org.junit.Test;
import org.junit.runner.RunWith;
import static org.junit.Assert.*;

@RunWith(AndroidJUnit4.class)
public class AppLaunchInstrumentedTest {
    @Test
    public void activityLaunchesAndShowsHeaderAndComposer() {
        ActivityScenario<MainActivity> scenario = ActivityScenario.launch(MainActivity.class);
        try {
            scenario.onActivity(activity -> {
                View root = activity.findViewById(android.R.id.content);
                assertNotNull("Activity content root should exist", root);
                assertTrue("Saarthi Studio heading should exist", containsText(root, "Saarthi Studio"));
                assertTrue(
                    "Message composer EditText with expected hint should exist",
                    containsEditTextWithHint(root, "Message Saarthi…")
                );
            });
        } finally {
            scenario.close();
        }
    }

    private static boolean containsText(View view, String expected) {
        if (view instanceof TextView && expected.contentEquals(((TextView) view).getText())) {
            return true;
        }
        if (view instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) view;
            for (int i = 0; i < group.getChildCount(); i++) {
                if (containsText(group.getChildAt(i), expected)) return true;
            }
        }
        return false;
    }

    private static boolean containsEditTextWithHint(View view, String expectedHint) {
        if (view instanceof EditText) {
            EditText input = (EditText) view;
            CharSequence hint = input.getHint();
            if (input.getVisibility() == View.VISIBLE && hint != null && expectedHint.contentEquals(hint)) {
                return true;
            }
        }
        if (view instanceof ViewGroup) {
            ViewGroup group = (ViewGroup) view;
            for (int i = 0; i < group.getChildCount(); i++) {
                if (containsEditTextWithHint(group.getChildAt(i), expectedHint)) return true;
            }
        }
        return false;
    }
}
