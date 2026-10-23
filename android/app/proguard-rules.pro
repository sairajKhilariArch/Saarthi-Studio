# Saarthi Studio — R8/ProGuard rules for the release build.
#
# The app's main source set has no runtime dependencies outside the Android
# framework and uses no reflection, JNI, runtime annotation processing, or
# framework-serialized classes, so the AGP default optimize rules
# (proguard-android-optimize.txt) are sufficient on their own.
#
# Add a -keep rule here only together with a regression test that proves the
# minified release build breaks without it.
