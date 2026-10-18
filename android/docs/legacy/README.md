# Legacy Android upgrade scripts and historical notes

These files are retained for reference only. They were written for the old layout where the Android Gradle project lived at the repository root and the current branch was named `android`.

**Do not run these scripts against the monorepo.** They expect paths such as `app/src/main/java/...` at the root and may overwrite files. Current Android sources live at `android/app/`. Make future changes directly on a reviewed feature branch, then run the Android CI workflow.
