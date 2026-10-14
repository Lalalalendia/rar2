//! One real Publisher PUB + one genuine OFL physical font through canonical
//! EditorSession authoring, native history, persistence and strict re-admission.
//! This is deliberately not text shaping, native PUB font write or fixed PDF.
use chaptera_text_format_overlay::{
    FontAuthoringScopeV1, FontReplacementCandidateV1, FontResourceIdentityV1,
    FormatPropertyV1, FormatValueV1, ServerFontResourceV1,
};
use pub_editor::{
    EditOperation, EditorProject, EditorProjectFontReopenGrantV1, Sha256Digest,
    StoryId, open_mature_0x2c_editor,
};
use sha2::{Digest, Sha256};
use std::{collections::BTreeMap, env, fs, path::PathBuf};

const ABEL_SHA256: &str = "8809dcad25318225052f88333e208c5aad4adcb7b2c934c135735ec19aa410b4";
const ABEL_UUID: &str = "f27a8036-8492-480f-8fa6-d2e775cc9f12";
const NEWSLETTER_SHA256: &str = "6a825ba26ba35d6e885acdc62e859591ed37cb0ff7480b554b9cb362b644dfcf";

fn exact_resource() -> Vec<u8> {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../assets/fonts/ofl/abel/Abel-Regular.ttf");
    let bytes = fs::read(path).expect("complete original pinned SIL OFL 1.1 Abel file");
    assert_eq!(bytes.len(), 35_220);
    assert_eq!(format!("{:x}", Sha256::digest(&bytes)), ABEL_SHA256);
    assert_eq!(&bytes[..4], &0x0001_0000_u32.to_be_bytes());
    bytes
}

fn resource_identity() -> FontResourceIdentityV1 {
    FontResourceIdentityV1 {
        resource_id: ABEL_UUID.to_owned(),
        font_fingerprint: format!("sha256:{ABEL_SHA256}"),
        content_hash: ABEL_SHA256.to_owned(),
        face_index: 0,
    }
}

fn admission<'a>(identity: &'a FontResourceIdentityV1, bytes: &'a [u8]) -> ServerFontResourceV1<'a> {
    ServerFontResourceV1 {
        identity,
        full_font_bytes: bytes,
        face_count: 1, // pinned independent Python sfnt verifier tested by #2512
        is_full_resource: true,
        authoring_admitted: true, // explicit pinned OFL witness only
    }
}

fn source_hash(bytes: &[u8]) -> Sha256Digest {
    let result: [u8; 32] = Sha256::digest(bytes).into();
    Sha256Digest::from_bytes(result)
}

#[test]
fn real_pub_admitted_full_font_enters_canonical_history_and_survives_fresh_reopen() {
    let Some(path) = env::var_os("CHAPTERA_REAL_PUB_FONT_FIXTURE") else {
        eprintln!("CHAPTERA_REAL_PUB_FONT_FIXTURE not supplied; dedicated CI provides pinned real PUB");
        return;
    };
    // cargo test runs with the package directory as cwd, not the repository root.
    // Resolve the pinned fixture relative to this existing tooling workspace.
    let fixture = PathBuf::from(path);
    let fixture = if fixture.is_absolute() {
        fixture
    } else {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..").join(fixture)
    };
    let source = fs::read(&fixture).expect("read exact pinned real PUB");
    assert_eq!(source.len(), 291_840, "not the pinned real Newsletter fixture");
    assert_eq!(format!("{:x}", Sha256::digest(&source)), NEWSLETTER_SHA256);
    let digest = source_hash(&source);

    let mut editor = open_mature_0x2c_editor(&source, digest).expect("real source-backed EditorSession");
    let original_project = editor.project();
    let doc_id = original_project.identity.as_ref()
        .expect("identity-bearing project is essential for physical font history")
        .document_id.clone();
    let ordered_stories = editor.graph().stories.keys().copied().collect::<Vec<StoryId>>();
    let mut inspected = 0usize;
    let viable = ordered_stories.into_iter().find_map(|story_id| {
        inspected += 1;
        let text = editor.graph().stories.get(&story_id)?.text.as_str();
        if text.chars().count() < 1 { return None; }
        let before = editor.current_text_format_overlay_v1(story_id).ok()?;
        let before_hash = editor.current_text_format_state_hash_v1(story_id).ok()?;
        Some((story_id, text.to_owned(), before, before_hash))
    });
    let (story_id, text_before, overlay_before, expected_hash) = viable.unwrap_or_else(|| {
        panic!("no exact source Story admitted complete text-format overlay among {inspected} real PUB Stories")
    });

    let font_bytes = exact_resource();
    let identity = resource_identity();
    let scope = FontAuthoringScopeV1 {
        document_id: doc_id.clone(),
        revision_id: format!("sha256:{}", "1".repeat(64)),
        scene_snapshot_id: format!("sha256:{}", "2".repeat(64)),
        layout_environment_id: format!("sha256:{}", "3".repeat(64)),
        font_set_fingerprint: format!("sha256:{}", "4".repeat(64)),
    };
    let candidate = FontReplacementCandidateV1 {
        protocol_version: "chaptera.font-replacement-candidate.v1".to_owned(),
        document_id: scope.document_id.clone(),
        expected_revision_id: scope.revision_id.clone(),
        scene_snapshot_id: scope.scene_snapshot_id.clone(),
        layout_environment_id: scope.layout_environment_id.clone(),
        font_set_fingerprint: scope.font_set_fingerprint.clone(),
        resource_id: identity.resource_id.clone(),
        font_fingerprint: identity.font_fingerprint.clone(),
        content_hash: identity.content_hash.clone(),
        face_index: identity.face_index,
        authority: "candidate_only_server_validation_required".to_owned(),
    };
    let trusted = admission(&identity, &font_bytes);

    // Neither a stale browser candidate nor forged bytes can create an operation.
    let mut stale = candidate.clone();
    stale.scene_snapshot_id = format!("sha256:{}", "9".repeat(64));
    assert!(editor.set_admitted_font_resource_v1(
        story_id, 0, 1, &stale, &scope, &trusted, &expected_hash
    ).is_err());
    let mut corrupt = font_bytes.clone();
    corrupt[32] ^= 1;
    assert!(editor.set_admitted_font_resource_v1(
        story_id, 0, 1, &candidate, &scope, &admission(&identity, &corrupt), &expected_hash,
    ).is_err());
    assert!(editor.operations().is_empty());

    let op = editor.set_admitted_font_resource_v1(
        story_id, 0, 1, &candidate, &scope, &trusted, &expected_hash,
    ).expect("genuine pinned font admitted into real PUB Story history");
    assert!(matches!(
        &op,
        EditOperation::SetTextFormatProperty {
            property: FormatPropertyV1::FontResource,
            value: FormatValueV1::FontResource(actual),
            start_scalar: 0,
            end_scalar: 1,
            ..
        } if actual == &identity
    ));
    assert_eq!(editor.graph().stories[&story_id].text, text_before);
    let after = editor.current_text_format_overlay_v1(story_id)
        .expect("effective physical font override on actual PUB Story");
    assert_ne!(after, overlay_before);
    assert_eq!(editor.operations().len(), 1);
    editor.undo().expect("Undo actual pinned PUB font choice");
    assert_eq!(editor.current_text_format_overlay_v1(story_id).unwrap(), overlay_before);
    editor.redo().expect("Redo actual pinned PUB font choice");
    assert_eq!(editor.current_text_format_overlay_v1(story_id).unwrap(), after);

    let project = editor.project();
    let persisted = serde_json::to_vec(&project).expect("durable canonical Project JSON");
    let loaded: EditorProject = serde_json::from_slice(&persisted).expect("fresh Project JSON");
    assert_eq!(loaded.operations.len(), 1);
    assert_eq!(loaded.source_hash, digest);
    assert_eq!(loaded.identity.as_ref().unwrap().document_id, doc_id);

    // Public unsafe reopen must remain blocked. Full bytes, source hash,
    // project document identity and authoring grant are rechecked independently.
    let mut denied = open_mature_0x2c_editor(&source, digest).unwrap();
    assert!(denied.apply_project(&loaded).is_err());
    assert!(denied.operations().is_empty());
    let grant = || EditorProjectFontReopenGrantV1 {
        source_hash: digest,
        project_document_id: &doc_id,
        resource: admission(&identity, &font_bytes),
    };
    let mut fresh = open_mature_0x2c_editor(&source, digest)
        .expect("fresh real Publisher Reader/Editor reopen");
    fresh.apply_project_with_admitted_font_resources_v1(
        &loaded, &BTreeMap::new(), &[grant()],
    ).expect("independently re-admitted original physical bytes on fresh PUB reopen");
    assert_eq!(fresh.current_text_format_overlay_v1(story_id).unwrap(), after);
    assert_eq!(fresh.project().state_id_v1(), loaded.state_id_v1());
    assert_eq!(fresh.graph().stories[&story_id].text, text_before);
    fresh.undo().expect("fresh session Undo");
    assert_eq!(fresh.current_text_format_overlay_v1(story_id).unwrap(), overlay_before);
    fresh.redo().expect("fresh session Redo");
    assert_eq!(fresh.current_text_format_overlay_v1(story_id).unwrap(), after);

    let mut wrong_source = open_mature_0x2c_editor(&source, digest).unwrap();
    assert!(wrong_source.apply_project_with_admitted_font_resources_v1(
        &loaded, &BTreeMap::new(), &[EditorProjectFontReopenGrantV1 {
            source_hash: Sha256Digest::from_bytes([0x11; 32]),
            ..grant()
        }]
    ).is_err());
    assert!(wrong_source.operations().is_empty());
    let mut wrong_font = open_mature_0x2c_editor(&source, digest).unwrap();
    assert!(wrong_font.apply_project_with_admitted_font_resources_v1(
        &loaded, &BTreeMap::new(), &[EditorProjectFontReopenGrantV1 {
            resource: admission(&identity, &corrupt),
            ..grant()
        }]
    ).is_err());
    assert!(wrong_font.operations().is_empty());
    println!(
        "REAL_PUB_FONT_HISTORY_OK story={story_id:?} inspected={inspected} original_sha256={NEWSLETTER_SHA256} font_sha256={ABEL_SHA256} project_state={}",
        loaded.state_id_v1(),
    );
}
