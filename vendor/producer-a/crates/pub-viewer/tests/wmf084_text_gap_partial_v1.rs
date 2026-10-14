//! Real pinned-source Viewer acceptance. Source bytes never enter artifacts.
//! Set CHAPTERA_PINNED_SOURCE_ROOT only on an authorized hosted fixture job.
use pub_viewer::{open_pub_geometry, viewer_geometry_environment_v0_1};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn pinned(root: &str, hash: &str) -> Vec<u8> {
    let bytes = fs::read(Path::new(root).join(format!("{hash}.pub")))
        .expect("exact Publisher fixture exists");
    let actual = Sha256::digest(&bytes)
        .iter()
        .map(|x| format!("{x:02x}"))
        .collect::<String>();
    assert_eq!(actual, hash);
    bytes
}

#[test]
fn exact_084_recovers_nine_partial_wmf_graphics_and_surfaces_nineteen_omitted_characters() {
    let Ok(root) = std::env::var("CHAPTERA_PINNED_SOURCE_ROOT") else {
        return;
    };
    let bytes = pinned(
        &root,
        "6e50032567c85a83cacc4f07520a2966411950c73a739029862417dec369908d",
    );
    let visual = open_pub_geometry(&bytes, viewer_geometry_environment_v0_1())
        .expect("real 084 source opens in Viewer");
    let partial = visual
        .document
        .diagnostics
        .iter()
        .filter(|d| d.code == "viewer.mature_officeart_wmf.visible_text_omitted_partial")
        .collect::<Vec<_>>();
    assert_eq!(
        partial.len(),
        1,
        "all per-source omissions must be consolidated"
    );
    assert!(partial[0].message.contains("9 bounded WMF previews"));
    assert!(partial[0].message.contains("19 visible source text record"));
    let candidates = visual
        .images
        .iter()
        .filter(|image| !image.source_exact && image.mime == "image/png")
        .collect::<Vec<_>>();
    assert!(
        candidates.len() >= 9,
        "real derived image resources must reach Viewer"
    );
    for image in candidates {
        assert!(image.verified_source_sha256.is_none());
        assert!(image.bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]));
    }
}

#[test]
fn exact_040_and_085_do_not_acquire_false_partial_text_warnings() {
    let Ok(root) = std::env::var("CHAPTERA_PINNED_SOURCE_ROOT") else {
        return;
    };
    for hash in [
        "b384bf6461c77c3d0fa227c148e1e3092cf69c98b0ad568a7b3e32fb139b6282",
        "40453fa60fbd9176f85e3b4d29f5074139b4e4a882d555d2801ec4a1a6c5fb62",
    ] {
        let bytes = pinned(&root, hash);
        let visual = open_pub_geometry(&bytes, viewer_geometry_environment_v0_1())
            .expect("WMF control source opens in Viewer");
        assert!(
            !visual
                .document
                .diagnostics
                .iter()
                .any(|d| d.code == "viewer.mature_officeart_wmf.visible_text_omitted_partial"),
            "text-free WMF control must retain strict existing admission"
        );
    }
}
