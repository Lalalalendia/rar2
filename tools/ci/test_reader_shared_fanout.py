#!/usr/bin/env python3
from reader_shared_fanout import classify

def expect(paths, **wanted):
    got = classify(paths)
    for key, value in wanted.items():
        assert got[key] is value, (paths, key, got)

expect(
    ["vendor/producer-a/crates/pub-contents/src/lib.rs"],
    reader_windows=True,
    editor_windows=False,
    visual_oracle=False,
    typography=False,
    corpus=False,
    android_render=False,
    web_acceptance=False,
)
expect(
    ["vendor/producer-a/crates/pub-viewer/src/lib.rs"],
    reader_windows=True,
    editor_windows=False,
    visual_oracle=True,
    typography=True,
    corpus=True,
    android_render=False,
    web_acceptance=True,
)
expect(
    ["crates/chaptera-viewer-render-plan/src/lib.rs"],
    reader_windows=True,
    editor_windows=True,
    visual_oracle=True,
    typography=True,
    corpus=True,
    android_render=True,
    web_acceptance=False,
)
expect(
    ["apps/chaptera-desktop/src/main.rs"],
    reader_windows=True,
    editor_windows=True,
    visual_oracle=True,
    typography=True,
    corpus=True,
    android_render=False,
    web_acceptance=False,
)
expect(
    ["apps/chaptera-mobile-android/app/src/main/AndroidManifest.xml"],
    reader_windows=False,
    editor_windows=False,
    visual_oracle=False,
    typography=False,
    corpus=False,
    android_render=False,
    web_acceptance=False,
)
print("reader shared fanout classifier tests: ok")

expect(
    [".github/workflows/mobile-reader-android-local-open.yml"],
    android_local=True,
    android_render=False,
    reader_windows=False,
)
expect(
    [".github/workflows/chaptera-local-portable-windows.yml"],
    local_portable=True,
    reader_windows=False,
)
expect(
    [".github/workflows/chaptera-reader-windows.yml"],
    reader_windows=True,
    editor_windows=False,
)
