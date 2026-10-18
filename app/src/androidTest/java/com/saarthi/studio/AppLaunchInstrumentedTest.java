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
 @Test public void activityLaunchesAndShowsHeaderAndComposer() {
  ActivityScenario<MainActivity> scenario=ActivityScenario.launch(MainActivity.class);
  try {
   scenario.onActivity(activity -> {
    View root=activity.findViewById(android.R.id.content);
    assertNotNull(root);
    assertTrue("Saarthi Studio heading should be visible",containsText(root,"Saarthi Studio"));
    assertTrue("Message composer should be visible with the expected hint",
      containsVisibleEditTextWithHint(root,"Message Saarthi…"));
   });
  } finally { scenario.close(); }
 }
 private static boolean containsText(View view,String expected) {
  if(view instanceof TextView && expected.contentEquals(((TextView)view).getText())) return true;
  if(view instanceof ViewGroup) {
   ViewGroup g=(ViewGroup)view;
   for(int i=0;i<g.getChildCount();i++) if(containsText(g.getChildAt(i),expected)) return true;
  }
  return false;
 }

 private static boolean containsVisibleEditTextWithHint(View view,String expectedHint) {
  if(view instanceof EditText) {
   EditText input=(EditText)view;
   if(input.getVisibility()==View.VISIBLE && input.isShown()
      && expectedHint.contentEquals(input.getHint())) return true;
  }
  if(view instanceof ViewGroup) {
   ViewGroup group=(ViewGroup)view;
   for(int i=0;i<group.getChildCount();i++) {
    if(containsVisibleEditTextWithHint(group.getChildAt(i),expectedHint)) return true;
   }
  }
  return false;
 }
}
