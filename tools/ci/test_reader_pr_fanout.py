# CI control: Reader close lifecycle cancellation proof.
#!/usr/bin/env python3
from __future__ import annotations

import importlib.util
from pathlib import Path

MODULE = Path(__file__).with_name("reader_pr_fanout.py")
spec = importlib.util.spec_from_file_location("reader_pr_fanout", MODULE)
mod = importlib.util.module_from_spec(spec)
assert spec and spec.loader
spec.loader.exec_module(mod)


def assert_scope(paths, *, evidence_only_paths=None, visual_neutral_paths=None, **expected):
    actual = mod.classify(paths, evidence_only_paths, visual_neutral_paths=visual_neutral_paths)
    for key, value in expected.items():
        assert actual[key] is value, (paths, key, actual)


def main():
    production = 'fn paint() { draw(1); }\n'
    comment_only = '// bounded neutral control\n' + production
    assert mod.desktop_main_visual_change_is_neutral(production, comment_only)
    assert not mod.desktop_main_visual_change_is_neutral(production, production.replace('draw(1)', 'draw(2)'))
    assert not mod.desktop_main_visual_change_is_neutral(production, '/// documentation\n' + production)
    for literal in (
        'const TEXT: &str = r##"\n// visible text\n"##;\n',
        'const TEXT: &str = br#"\n// visible text\n"#;\n',
        'const TEXT: &str = cr#"\n// visible text\n"#;\n',
        'const TEXT: &str = "\n// visible text\n";\n',
        '/* outer /* nested */\n// retained block content\n*/\n',
    ):
        assert not mod.desktop_main_visual_change_is_neutral(literal, literal.replace('//', '// changed'))
        assert mod.desktop_main_visual_change_is_neutral(literal, '// neutral\n' + literal)
    for malformed in ('r#"unterminated\n// text\n', '"unterminated\n// text\n', '/* unterminated\n// text\n'):
        assert not mod.desktop_main_visual_change_is_neutral(malformed, '// neutral\n' + malformed)
    chars_and_lifetimes = "fn f<'a>(x: &'a str) { let slash = '/'; let quote = '\\''; }\n"
    assert mod.desktop_main_visual_change_is_neutral(chars_and_lifetimes, '// neutral\n' + chars_and_lifetimes)
    assert not mod.desktop_main_visual_change_is_neutral(chars_and_lifetimes, chars_and_lifetimes.replace("'/'", "'*'"))

    base_source = """fn production() {}

#[cfg(test)]
mod tests {
    #[test]
    fn probe() {}
}
"""
    head_source = """fn production() {}

#[cfg(test)]
mod tests {
    #[test]
    fn probe() {
        eprintln!("evidence");
    }
}
"""
    old_marker = mod.cfg_test_module_line(base_source)
    new_marker = mod.cfg_test_module_line(head_source)
    assert old_marker == 4 and new_marker == 4
    assert not mod.diff_hunks_within_test_region(
        "@@ -4,1 +4,1 @@\n-mod tests {\n+mod tests { // evidence marker change\n",
        old_test_line=old_marker,
        new_test_line=new_marker,
    )
    assert mod.diff_hunks_within_test_region(
        "@@ -6,1 +6,3 @@\n-    fn probe() {}\n+    fn probe() {\n+        eprintln!(\"evidence\");\n+    }\n",
        old_test_line=old_marker,
        new_test_line=new_marker,
    )
    assert not mod.diff_hunks_within_test_region(
        "@@ -1,1 +1,1 @@\n-fn production() {}\n+fn production() { eprintln!(\"runtime\"); }\n",
        old_test_line=old_marker,
        new_test_line=new_marker,
    )


    valid_dll_bootstrap = """#[cfg(target_os = "windows")]
mod windows_dll_search;

fn main() -> eframe::Result<()> {
    #[cfg(target_os = "windows")]
    if let Err(error) = windows_dll_search::install_process_policy() {
        eprintln!("failed to establish safe DLL search policy: {error}");
        std::process::exit(2);
    }

    let mut args = std::env::args_os().skip(1).peekable();
}
"""
    assert mod.windows_dll_bootstrap_errors(valid_dll_bootstrap) == []
    assert mod.windows_dll_bootstrap_errors(
        valid_dll_bootstrap.replace(
            '#[cfg(target_os = "windows")]\nmod windows_dll_search;\n\n', ""
        )
    )
    assert mod.windows_dll_bootstrap_errors(
        valid_dll_bootstrap.replace(
            "windows_dll_search::install_process_policy()", "Ok::<(), String>(())"
        )
    )
    assert mod.windows_dll_bootstrap_errors(
        valid_dll_bootstrap.replace("std::process::exit(2);", "return Ok(());")
    )

    valid_reader_cli_bootstrap = """mod reader_product_cli;

fn main() -> eframe::Result<()> {
    let mut args = std::env::args_os().skip(1).peekable();
    let first_arg = args.next();
    if reader_product_cli::try_handle_product_smoke(first_arg.as_deref(), &mut args) {
        return Ok(());
    }
    if reader_product_cli::try_handle_reader_probe(
        first_arg.as_deref(),
        &mut args,
        reader_only_mode(),
    ) {
        return Ok(());
    }
    let initial_path = first_arg.map(PathBuf::from);
}
"""
    assert mod.reader_product_cli_bootstrap_errors(valid_reader_cli_bootstrap) == []
    assert mod.reader_product_cli_bootstrap_errors(
        valid_reader_cli_bootstrap.replace("mod reader_product_cli;\n", "")
    )
    assert mod.reader_product_cli_bootstrap_errors(
        valid_reader_cli_bootstrap.replace(
            "reader_product_cli::try_handle_product_smoke",
            "reader_product_cli::missing_product_smoke",
        )
    )
    assert mod.reader_product_cli_bootstrap_errors(
        valid_reader_cli_bootstrap.replace(
            "reader_product_cli::try_handle_reader_probe",
            "reader_product_cli::missing_reader_probe",
        )
    )
    assert_scope(
        ["apps/chaptera-server/src/reader_scene_v1.rs"],
        evidence_only_paths={"apps/chaptera-server/src/reader_scene_v1.rs"},
        local_portable=False,
        tier_a=False,
        reader_windows_smoke=False,
        reader_windows=False,
        editor_windows=False,
        visual_oracle=False,
        typography_golden=False,
        android_core=False,
        android=False,
        web=False,
    )

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
        ["vendor/producer-a/crates/pub-quill/src/typography.rs"],
        tier_a=True,
        reader_windows_smoke=True,
        reader_windows=False,
        editor_windows=False,
        visual_oracle=True,
        typography_golden=True,
        android_core=True,
        android=False,
        web=False,
        local_portable=False,
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
        ["apps/cloud-reader/render-v1.mjs"],
        tier_a=False,
        cloud_reference=True,
        virginia_page_role=False,
        visual_batch01=True,
    )
    assert_scope(
        ["tools/validate_virginia_page_roles.py"],
        tier_a=False,
        cloud_reference=False,
        virginia_page_role=True,
        visual_batch01=False,
    )
    assert_scope(
        ["tools/cloud_reader_visual_fingerprint_v1.py"],
        tier_a=False,
        cloud_reference=False,
        virginia_page_role=False,
        visual_batch01=True,
    )
    assert_scope(
        ["vendor/producer-a/crates/pub-reader/src/lib.rs"],
        cloud_reference=True,
        virginia_page_role=True,
        visual_batch01=True,
    )
    assert_scope(
        ["vendor/producer-a/crates/pub-reader/src/bin/source_image_export_probe.rs"],
        cloud_reference=False,
        virginia_page_role=False,
        visual_batch01=False,
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
        ["apps/chaptera-desktop/src/main.rs"],
        tier_a=True,
        reader_windows_smoke=True,
        reader_windows=False,
        editor_windows=False,
        visual_oracle=True,
        typography_golden=True,
        update_accept=False,
    )
    assert_scope(
        [mod.DESKTOP_MAIN],
        visual_neutral_paths={mod.DESKTOP_MAIN},
        tier_a=True,
        reader_windows_smoke=True,
        visual_oracle=False,
        typography_golden=False,
    )
    assert_scope(
        [mod.DESKTOP_MAIN, "apps/chaptera-desktop/src/render_backend.rs"],
        visual_neutral_paths={mod.DESKTOP_MAIN},
        visual_oracle=True,
        typography_golden=True,
    )
    assert_scope(
        [mod.DESKTOP_MAIN, "apps/chaptera-desktop/src/reader_visual_golden_tests.rs"],
        visual_neutral_paths={mod.DESKTOP_MAIN},
        visual_oracle=True,
        typography_golden=True,
    )
    assert_scope(
        ["apps/chaptera-desktop/src/reader_visual_golden_tests.rs"],
        tier_a=True,
        reader_windows_smoke=False,
        reader_windows=False,
        editor_windows=False,
        visual_oracle=True,
        typography_golden=True,
        update_accept=False,
    )
    assert_scope(
        ["apps/chaptera-desktop/src/reader_product_cli.rs"],
        tier_a=True,
        reader_windows_smoke=False,
        reader_windows=True,
        editor_windows=False,
        visual_oracle=False,
        typography_golden=False,
        update_accept=False,
    )
    assert_scope(
        ["apps/chaptera-desktop/src/reader_update_control.rs"],
        tier_a=True,
        reader_windows_smoke=False,
        reader_windows=True,
        editor_windows=False,
        update_accept=True,
    )
    assert_scope(
        [
            "apps/chaptera-desktop/src/main.rs",
            "apps/chaptera-desktop/src/ordinary_feature_module.rs",
        ],
        tier_a=True,
        reader_windows_smoke=True,
        reader_windows=False,
        editor_windows=True,
        visual_oracle=True,
        typography_golden=True,
        update_accept=False,
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
