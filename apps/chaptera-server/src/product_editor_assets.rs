use axum::{
    Router,
    body::{Body, Bytes},
    http::{
        HeaderName, HeaderValue, StatusCode,
        header::{CACHE_CONTROL, CONTENT_TYPE},
    },
    response::Response,
    routing::get,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};

const INDEX_HTML: &[u8] = include_bytes!("../../web/product-editor.html");
const EDITOR_CSS: &[u8] = include_bytes!("../../web/product-editor.css");
const EDITOR_ENTRY: &[u8] = include_bytes!("../../web/product-editor-entry-v1.mjs");
const PRODUCT_SERVICE: &[u8] = include_bytes!("../../web/chaptera-product-editor-service-v1.mjs");
const RICH_SHELL: &[u8] = include_bytes!("../../web/rich-reader-editor-shell-v1.mjs");
const INTERACTION_SCENE: &[u8] = include_bytes!("../../web/reader-scene-editor-interaction-v1.mjs");
const INTERACTION: &[u8] = include_bytes!("../../web/interaction-v1.mjs");
const OBSERVABILITY: &[u8] = include_bytes!("../../web/observability-v1.mjs");
const READER_ADAPTER: &[u8] = include_bytes!("../../web/reader-scene-editor-adapter-v1.mjs");
const READER_RENDER: &[u8] = include_bytes!("../../cloud-reader/render-v1.mjs");

const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data: blob:; font-src 'self' data: blob:; connect-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'self'";
const PERMISSIONS_POLICY: &str = "camera=(), microphone=(), geolocation=()";

struct EmbeddedAsset {
    name: &'static str,
    content_type: &'static str,
    bytes: &'static [u8],
}

const ASSETS: [EmbeddedAsset; 10] = [
    EmbeddedAsset {
        name: "product-editor.html",
        content_type: "text/html; charset=utf-8",
        bytes: INDEX_HTML,
    },
    EmbeddedAsset {
        name: "product-editor.css",
        content_type: "text/css; charset=utf-8",
        bytes: EDITOR_CSS,
    },
    EmbeddedAsset {
        name: "product-editor-entry-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: EDITOR_ENTRY,
    },
    EmbeddedAsset {
        name: "chaptera-product-editor-service-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: PRODUCT_SERVICE,
    },
    EmbeddedAsset {
        name: "rich-reader-editor-shell-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: RICH_SHELL,
    },
    EmbeddedAsset {
        name: "reader-scene-editor-interaction-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: INTERACTION_SCENE,
    },
    EmbeddedAsset {
        name: "interaction-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: INTERACTION,
    },
    EmbeddedAsset {
        name: "observability-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: OBSERVABILITY,
    },
    EmbeddedAsset {
        name: "reader-scene-editor-adapter-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: READER_ADAPTER,
    },
    EmbeddedAsset {
        name: "cloud-reader/render-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: READER_RENDER,
    },
];

pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/editor/doc/{document_id}", get(index))
        .route("/editor/product-editor.css", get(editor_css))
        .route("/editor/product-editor-entry-v1.mjs", get(editor_entry))
        .route(
            "/editor/chaptera-product-editor-service-v1.mjs",
            get(product_service),
        )
        .route("/editor/rich-reader-editor-shell-v1.mjs", get(rich_shell))
        .route(
            "/editor/reader-scene-editor-interaction-v1.mjs",
            get(interaction_scene),
        )
        .route("/editor/interaction-v1.mjs", get(interaction))
        .route("/editor/observability-v1.mjs", get(observability))
        .route(
            "/editor/reader-scene-editor-adapter-v1.mjs",
            get(reader_adapter),
        )
        .route("/cloud-reader/render-v1.mjs", get(reader_render))
}

pub fn version_manifest() -> Value {
    let files = ASSETS
        .iter()
        .map(|asset| {
            json!({
                "name": asset.name,
                "byte_len": asset.bytes.len(),
                "sha256": format!("{:x}", Sha256::digest(asset.bytes)),
            })
        })
        .collect::<Vec<_>>();
    json!({ "embedded": true, "asset_count": ASSETS.len(), "files": files })
}

async fn index() -> Response {
    asset_response(&ASSETS[0])
}
async fn editor_css() -> Response {
    asset_response(&ASSETS[1])
}
async fn editor_entry() -> Response {
    asset_response(&ASSETS[2])
}
async fn product_service() -> Response {
    asset_response(&ASSETS[3])
}
async fn rich_shell() -> Response {
    asset_response(&ASSETS[4])
}
async fn interaction_scene() -> Response {
    asset_response(&ASSETS[5])
}
async fn interaction() -> Response {
    asset_response(&ASSETS[6])
}
async fn observability() -> Response {
    asset_response(&ASSETS[7])
}
async fn reader_adapter() -> Response {
    asset_response(&ASSETS[8])
}
async fn reader_render() -> Response {
    asset_response(&ASSETS[9])
}

fn asset_response(asset: &EmbeddedAsset) -> Response {
    let mut response = Response::new(Body::from(Bytes::from_static(asset.bytes)));
    *response.status_mut() = StatusCode::OK;
    let headers = response.headers_mut();
    headers.insert(CONTENT_TYPE, HeaderValue::from_static(asset.content_type));
    headers.insert(CACHE_CONTROL, HeaderValue::from_static("no-cache"));
    headers.insert(
        HeaderName::from_static("content-security-policy"),
        HeaderValue::from_static(CONTENT_SECURITY_POLICY),
    );
    headers.insert(
        HeaderName::from_static("x-content-type-options"),
        HeaderValue::from_static("nosniff"),
    );
    headers.insert(
        HeaderName::from_static("referrer-policy"),
        HeaderValue::from_static("no-referrer"),
    );
    headers.insert(
        HeaderName::from_static("permissions-policy"),
        HeaderValue::from_static(PERMISSIONS_POLICY),
    );
    response
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    const DOC: &str = "10000000-0000-7000-8000-000000000001";

    #[tokio::test]
    async fn editor_document_route_serves_source_neutral_product_entry() {
        let response = router::<()>()
            .oneshot(
                Request::builder()
                    .uri(format!("/editor/doc/{DOC}"))
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers()["content-type"],
            "text/html; charset=utf-8"
        );
        assert!(
            response.headers()["content-security-policy"]
                .to_str()
                .unwrap()
                .contains("frame-ancestors 'none'")
        );
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        let html = std::str::from_utf8(&bytes).unwrap();
        assert!(html.contains("/editor/product-editor-entry-v1.mjs"));
        assert!(!html.contains("synthetic-editor"));
        assert!(!html.contains("x-chaptera-principal-id"));
    }

    #[tokio::test]
    async fn product_service_reader_adapter_import_is_physically_served() {
        let service = std::str::from_utf8(PRODUCT_SERVICE).unwrap();
        assert!(service.contains("./reader-scene-editor-adapter-v1.mjs"));
        assert!(service.contains("./reader-scene-editor-interaction-v1.mjs"));

        let response = router::<()>()
            .oneshot(
                Request::builder()
                    .uri("/editor/reader-scene-editor-adapter-v1.mjs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(bytes.as_ref(), READER_ADAPTER);
    }

    #[tokio::test]
    async fn editor_assets_include_shared_reader_renderer_alias() {
        let response = router::<()>()
            .oneshot(
                Request::builder()
                    .uri("/cloud-reader/render-v1.mjs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(bytes.as_ref(), READER_RENDER);
    }

    #[test]
    fn manifest_covers_every_embedded_editor_asset() {
        let manifest = version_manifest();
        assert_eq!(manifest["asset_count"], ASSETS.len());
        assert_eq!(manifest["files"].as_array().unwrap().len(), ASSETS.len());
    }
}
