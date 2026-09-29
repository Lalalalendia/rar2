#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path

MODULE = Path(__file__).with_name("reader_pr_fanout.py")
spec = importlib.util.spec_from_file_location("reader_pr_fanout", MODULE)
mod = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(mod)


def assert_scope(paths, **expected):
    actual = mod.classify(paths)
    for key, value in expected.items():
        assert actual[key] is value, (paths, key, actual)


def main():
    assert_scope(
        ["vendor/producer-a/crates/pub-contents/src/palette.rs"],
        tier_a=True,
        reader_windows_smoke=True,
        reader_windows=False,
        editor_windows=False,
        android_core=True,
        android=False,
        web=False,
        local_portable=False,
        visual_oracle=False,
        typography_golden=False,
    )
    assert_scope(
        ["crates/pub-model/src/lib.rs"],
        tier_a=False,
        visual_oracle=True,
    )
    assert_scope(
        ["tools/reader_page_role_observation.py"],
        tier_a=False,
        visual_oracle=True,
    )
    assert_scope(
        ["vendor/producer-a/crates/pub-viewer/src/lib.rs"],
        tier_a=True,
        reader_windows_smoke=True,
        reader_windows=False,
        visual_oracle=True,
        typography_golden=True,
        android_core=True,
        android=False,
        web=False,
        local_portable=False,
    )
    assert_scope(
        ["crates/chaptera-viewer-render-plan/src/lib.rs"],
        tier_a=True,
        reader_windows_smoke=True,
        reader_windows=False,
        editor_windows=False,
        visual_oracle=True,
        typography_golden=True,
        android_core=True,
        android=False,
    )
    assert_scope(
        ["apps/chaptera-mobile-android/app/src/main/AndroidManifest.xml"],
        tier_a=False,
        android_core=False,
        android=True,
        reader_windows_smoke=False,
        reader_windows=False,
        editor_windows=False,
        web=False,
    )
    assert_scope(
        ["apps/web/editor-shell-http-harness.html"],
        tier_a=False,
        web=True,
        local_portable=True,
        android_core=False,
        android=False,
        reader_windows_smoke=False,
        reader_windows=False,
    )
    assert_scope(
        ["installer/windows/chaptera-reader.iss"],
        tier_a=False,
        installer=True,
        update_accept=True,
        reader_windows_smoke=False,
        reader_windows=False,
    )
    assert_scope(
        ["apps/chaptera-desktop/src/text_session.rs"],
        tier_a=True,
        editor_windows=True,
        reader_windows_smoke=False,
        reader_windows=False,
        android_core=False,
        android=False,
    )
    assert_scope(
        ["crates/chaptera-update-orchestrator/src/lib.rs"],
        tier_a=False,
        update_accept=True,
        reader_windows_smoke=False,
        reader_windows=True,
        installer=False,
    )
    assert_scope(
        ["vendor/producer-a/crates/pub-editor/src/lib.rs"],
        tier_a=True,
        reader_windows_smoke=False,
        reader_windows=False,
        editor_windows=False,
        android_core=False,
    )
    assert_scope(
        ["apps/chaptera-desktop/src/reader_product_ui.rs"],
        tier_a=True,
        reader_windows_smoke=False,
        reader_windows=True,
        editor_windows=False,
    )
    assert_scope(
        ["apps/chaptera-desktop/src/fallback_font.rs"],
        tier_a=True,
        reader_windows_smoke=False,
        reader_windows=False,
        editor_windows=False,
    )
    assert_scope(
        ["apps/chaptera-desktop/src/source_font.rs"],
        tier_a=True,
        reader_windows_smoke=False,
        reader_windows=False,
        editor_windows=False,
        visual_oracle=True,
        typography_golden=True,
    )
    assert_scope(
        ["vendor/producer-a/crates/pub-viewer/src/bin/corpus-reader-receipt.rs"],
        tier_a=False,
        reader_windows_smoke=False,
        reader_windows=False,
        editor_windows=False,
        visual_oracle=False,
        typography_golden=False,
        android_core=False,
        android=False,
        web=False,
        local_portable=False,
        installer=False,
        path_identity=False,
        update_accept=False,
    )
    assert_scope(
        [
            "vendor/producer-a/crates/pub-reader/src/lib.rs",
            "packages/product/reader-portable/v1/README.md",
        ],
        tier_a=True,
        reader_windows_smoke=False,
        reader_windows=True,
    )
    assert_scope(
        ["docs/notes.md"],
        tier_a=False,
        reader_windows_smoke=False,
        reader_windows=False,
        editor_windows=False,
        visual_oracle=False,
        typography_golden=False,
        android_core=False,
        android=False,
        web=False,
        local_portable=False,
        installer=False,
        path_identity=False,
        update_accept=False,
    )
    print("reader_pr_fanout tests: ok")


if __name__ == "__main__":
    main()
