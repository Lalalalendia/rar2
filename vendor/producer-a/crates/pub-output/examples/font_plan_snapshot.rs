use pub_layout::font_fingerprint_sha256;
use pub_output::{
    ExplicitFontResource, FixedOutputFontProfile, FontIdentity, OutputFontRequest,
    TechnicalEmbeddingFlags, plan_output_fonts,
};
use std::collections::BTreeSet;

fn main() {
    let bytes = font_test_data::NOTOSERIF_AUTOHINT_SHAPING;
    let identity = FontIdentity {
        fingerprint_sha256: font_fingerprint_sha256(bytes),
        face_index: 0,
    };
    let resource = ExplicitFontResource {
        identity: identity.clone(),
        bytes,
        embedding: TechnicalEmbeddingFlags::installable(),
    };
    let request = OutputFontRequest {
        source: identity,
        source_resource: Some(resource),
        fallback_resource: None,
        used_glyph_ids: BTreeSet::from([2, 4, 9]),
    };
    let plan = plan_output_fonts(&FixedOutputFontProfile::basic_pdf_v0_1(), vec![request]);
    println!(
        "{}",
        serde_json::to_string(&plan).expect("font plan snapshot must serialize")
    );
}
