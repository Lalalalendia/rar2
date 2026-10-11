//! Real corpus acceptance is opt-in: normal CI remains source-free.
//! CHAPTERA_PINNED_SOURCE_ROOT points to the SHA-named pinned1050 PUB corpus.
use pub_viewer::{open_pub_geometry, viewer_geometry_environment_v0_1};
use sha2::{Digest, Sha256};
use std::{fs, path::Path};

fn load_pinned(root: &str, sha: &str) -> Vec<u8> {
    let bytes = fs::read(Path::new(root).join(format!("{sha}.pub")))
        .expect("pinned PUB fixture readable");
    let actual_sha = Sha256::digest(&bytes)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    assert_eq!(actual_sha, sha);
    bytes
}

#[test]
fn exact_041_emf_bitmap_use_join_yields_six_derived_partial_resources() {
    let Ok(root) = std::env::var("CHAPTERA_PINNED_SOURCE_ROOT") else {
        return;
    };
    let source = load_pinned(
        &root,
        "155f8fd8c191851e4380ab1557638aeabb34b2257fa12a7748ab00b09edb74ba",
    );
    let visual = open_pub_geometry(&source, viewer_geometry_environment_v0_1())
        .expect("041 must open through real Viewer scene");
    let derived = visual.images.iter().filter(|image| !image.source_exact).collect::<Vec<_>>();
    assert_eq!(derived.len(), 6, "all six strict EMF bitmap slots must reach Viewer");
    assert_eq!(
        derived.iter().map(|image| image.node_ids.len()).sum::<usize>(),
        6,
        "exact source EMF uses must map one-to-one to Viewer scene nodes"
    );
    for image in derived {
        assert_eq!(image.mime, "image/png");
        assert!(image.bytes.starts_with(&[137, 80, 78, 71, 13, 10, 26, 10]));
        assert_eq!(image.node_ids.len(), 1);
        assert!(image.verified_source_sha256.is_none());
    }
    assert!(visual.document.diagnostics.iter().any(|diagnostic| {
        diagnostic.code == "viewer.mature_officeart_emf.bitmap_layer_partial"
    }));
}

#[test]
fn exact_066_vector_only_emf_never_becomes_bitmap_preview() {
    let Ok(root) = std::env::var("CHAPTERA_PINNED_SOURCE_ROOT") else {
        return;
    };
    let source = load_pinned(
        &root,
        "d8ac6507d86e35c0f43e293760627a01b00a29586a76f8bf05b5fae702a19d13",
    );
    let visual = open_pub_geometry(&source, viewer_geometry_environment_v0_1())
        .expect("066 must open through real Viewer scene");
    assert!(
        !visual.document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "viewer.mature_officeart_emf.bitmap_layer_partial"
        }),
        "vector-only 066 must not be promoted to bitmap"
    );
    assert!(
        visual.document.diagnostics.iter().any(|diagnostic| {
            diagnostic.code == "viewer.mature_officeart_emf.vector_not_rendered"
        }),
        "vector-only omission must remain visible"
    );
}
