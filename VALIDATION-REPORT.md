# Saarthi Studio Android 1.11.1 — Validation Report 

- Version: `1.11.1`, version code `14`; minimum Android SDK remains 26.
- API 27 navigation-bar appearance item moved into `values-v27/styles.xml`; the base theme is API-26 compatible.
- Persistence and data models moved to `ChatDatabase.java`; migration helpers are in `DatabaseMigrations.java`. Schema v6 records imported backup fingerprints.
- Restore path is transactional and only skips byte-for-byte identical previously imported backup files.
- 21 pure-Java regression test methods were executed with a lightweight local runner and passed; they cover backup schema versions/limits, attachment type/size/image payload policies, network retry/fallback rules, secret redaction, model selection, and search-query construction. This is not a substitute for running the Gradle JUnit task in CI.
- Instrumentation tests cover v4→v5→v6 migration preservation, duplicate/failed backup imports, parameterized search/filter SQL, bounded message-history paging, and app launch.
- GitHub Actions reports lint output, uploads the APK whenever assembleDebug succeeds, and runs emulator instrumentation tests before enforcing the final pass/fail gate.

## Not yet verified

This environment does not contain Gradle or an Android SDK, so no local Android Gradle build, actual lint run, APK install, or emulator test is claimed. The previous v1.11.0 Actions run failed at lint due to `android:windowLightNavigationBar` requiring API 27 under minSdk 26; this source moves that item to an API-27 resource folder. The previous lint run did not expose the 40 warning details in its logs and had no report artifact. The revised workflow uploads a lint-report artifact so those warnings can be inspected on the next run and meaningful issues fixed. Physical-device FreeLLMAPI, speech, vision, upgrade, and backup scenarios remain required.
