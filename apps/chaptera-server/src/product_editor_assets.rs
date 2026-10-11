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
const CREATE_HTML: &[u8] = include_bytes!("../../web/cloud-project-new.html");
const CREATE_CSS: &[u8] = include_bytes!("../../web/cloud-project-new.css");
const CREATE_ENTRY: &[u8] = include_bytes!("../../web/cloud-project-new-v1.mjs");
const HOME_HTML: &[u8] = include_bytes!("../../web/cloud-project-home.html");
const HOME_CSS: &[u8] = include_bytes!("../../web/cloud-project-home.css");
const HOME_ENTRY: &[u8] = include_bytes!("../../web/cloud-project-home-v1.mjs");
const PROJECT_RENAME_CLIENT: &[u8] = include_bytes!("../../web/cloud-project-rename-v1.mjs");
const PROJECT_HOME_CONTROLLER: &[u8] = include_bytes!("../../web/project-home-v1.mjs");
const PROJECT_CATALOG_CLIENT: &[u8] =
    include_bytes!("../../web/chaptera-cloud-project-catalog-v1.mjs");
const FILE_ENTRY: &[u8] = include_bytes!("../../web/file-entry-v1.mjs");
const WORKSPACE_SESSION_CLIENT: &[u8] =
    include_bytes!("../../web/chaptera-cloud-workspace-session-v1.mjs");
const SOURCE_INGRESS_CLIENT: &[u8] =
    include_bytes!("../../web/chaptera-cloud-source-ingress-v1.mjs");
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

const ASSETS: [EmbeddedAsset; 22] = [
    EmbeddedAsset {
        name: "product-editor.html",
        content_type: "text/html; charset=utf-8",
        bytes: INDEX_HTML,
    },
    EmbeddedAsset {
        name: "cloud-project-new.html",
        content_type: "text/html; charset=utf-8",
        bytes: CREATE_HTML,
    },
    EmbeddedAsset {
        name: "cloud-project-new.css",
        content_type: "text/css; charset=utf-8",
        bytes: CREATE_CSS,
    },
    EmbeddedAsset {
        name: "cloud-project-new-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: CREATE_ENTRY,
    },
    EmbeddedAsset {
        name: "cloud-project-home.html",
        content_type: "text/html; charset=utf-8",
        bytes: HOME_HTML,
    },
    EmbeddedAsset {
        name: "cloud-project-home.css",
        content_type: "text/css; charset=utf-8",
        bytes: HOME_CSS,
    },
    EmbeddedAsset {
        name: "cloud-project-home-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: HOME_ENTRY,
    },
    EmbeddedAsset {
        name: "project-home-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: PROJECT_HOME_CONTROLLER,
    },
    EmbeddedAsset {
        name: "chaptera-cloud-project-catalog-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: PROJECT_CATALOG_CLIENT,
    },
    EmbeddedAsset {
        name: "file-entry-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: FILE_ENTRY,
    },
    EmbeddedAsset {
        name: "chaptera-cloud-workspace-session-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: WORKSPACE_SESSION_CLIENT,
    },
    EmbeddedAsset {
        name: "chaptera-cloud-source-ingress-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: SOURCE_INGRESS_CLIENT,
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
    EmbeddedAsset {
        name: "cloud-project-rename-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: PROJECT_RENAME_CLIENT,
    },
];

pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/editor/doc/{document_id}", get(index))
        .route("/editor", get(project_home))
        .route("/editor/projects", get(project_home))
        .route("/editor/new", get(new_project))
        .route("/editor/cloud-project-home.css", get(project_home_css))
        .route("/editor/cloud-project-home-v1.mjs", get(project_home_entry))
        .route("/editor/cloud-project-rename-v1.mjs", get(project_rename_client))
        .route("/editor/project-home-v1.mjs", get(project_home_controller))
        .route(
            "/editor/chaptera-cloud-project-catalog-v1.mjs",
            get(project_catalog_client),
        )
        .route("/editor/cloud-project-new.css", get(create_css))
        .route("/editor/cloud-project-new-v1.mjs", get(create_entry))
        .route("/editor/file-entry-v1.mjs", get(file_entry))
        .route(
            "/editor/chaptera-cloud-workspace-session-v1.mjs",
            get(workspace_session_client),
        )
        .route(
            "/editor/chaptera-cloud-source-ingress-v1.mjs",
            get(source_ingress_client),
        )
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
async fn new_project() -> Response {
    asset_response(&ASSETS[1])
}
async fn project_home() -> Response {
    asset_response(&ASSETS[4])
}
async fn project_home_css() -> Response {
    asset_response(&ASSETS[5])
}
async fn project_home_entry() -> Response {
    asset_response(&ASSETS[6])
}
async fn project_rename_client() -> Response {
    asset_response(&ASSETS[21])
}
async fn project_home_controller() -> Response {
    asset_response(&ASSETS[7])
}
async fn project_catalog_client() -> Response {
    asset_response(&ASSETS[8])
}
async fn create_css() -> Response {
    asset_response(&ASSETS[2])
}
async fn create_entry() -> Response {
    asset_response(&ASSETS[3])
}
async fn file_entry() -> Response {
    asset_response(&ASSETS[9])
}
async fn workspace_session_client() -> Response {
    asset_response(&ASSETS[10])
}
async fn source_ingress_client() -> Response {
    asset_response(&ASSETS[11])
}
async fn editor_css() -> Response {
    asset_response(&ASSETS[12])
}
async fn editor_entry() -> Response {
    asset_response(&ASSETS[13])
}
async fn product_service() -> Response {
    asset_response(&ASSETS[14])
}
async fn rich_shell() -> Response {
    asset_response(&ASSETS[15])
}
async fn interaction_scene() -> Response {
    asset_response(&ASSETS[16])
}
async fn interaction() -> Response {
    asset_response(&ASSETS[17])
}
async fn observability() -> Response {
    asset_response(&ASSETS[18])
}
async fn reader_adapter() -> Response {
    asset_response(&ASSETS[19])
}
async fn reader_render() -> Response {
    asset_response(&ASSETS[20])
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

    #[tokio::test]
    async fn cloud_pub_new_project_route_serves_embedded_modules() {
        let routes = router::<()>();
        for (uri, expected_type, bytes) in [
            ("/editor/new", "text/html; charset=utf-8", CREATE_HTML),
            (
                "/editor/cloud-project-new.css",
                "text/css; charset=utf-8",
                CREATE_CSS,
            ),
            (
                "/editor/cloud-project-new-v1.mjs",
                "text/javascript; charset=utf-8",
                CREATE_ENTRY,
            ),
            (
                "/editor/file-entry-v1.mjs",
                "text/javascript; charset=utf-8",
                FILE_ENTRY,
            ),
            (
                "/editor/chaptera-cloud-workspace-session-v1.mjs",
                "text/javascript; charset=utf-8",
                WORKSPACE_SESSION_CLIENT,
            ),
            (
                "/editor/chaptera-cloud-source-ingress-v1.mjs",
                "text/javascript; charset=utf-8",
                SOURCE_INGRESS_CLIENT,
            ),
        ] {
            let response = routes
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            assert_eq!(response.headers()["content-type"], expected_type, "{uri}");
            let response_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            assert_eq!(response_bytes.as_ref(), bytes, "{uri}");
        }
        let html = std::str::from_utf8(CREATE_HTML).unwrap();
        assert!(html.contains("/editor/cloud-project-new-v1.mjs"));
        assert!(!html.contains("x-chaptera-principal-id"));
    }

    #[tokio::test]
    async fn project_rename_module_is_embedded_as_same_origin_csp_resource() {
        let response = router::<()>()
            .oneshot(
                Request::builder()
                    .uri("/editor/cloud-project-rename-v1.mjs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers()["content-type"], "text/javascript; charset=utf-8");
        assert!(response.headers()["content-security-policy"]
            .to_str().unwrap().contains("connect-src 'self'"));
        let bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(bytes.as_ref(), PROJECT_RENAME_CLIENT);
    }

    #[tokio::test]
    async fn cloud_project_home_route_serves_catalog_modules() {
        let routes = router::<()>();
        for (uri, expected_type, bytes) in [
            ("/editor", "text/html; charset=utf-8", HOME_HTML),
            ("/editor/projects", "text/html; charset=utf-8", HOME_HTML),
            (
                "/editor/cloud-project-home.css",
                "text/css; charset=utf-8",
                HOME_CSS,
            ),
            (
                "/editor/cloud-project-home-v1.mjs",
                "text/javascript; charset=utf-8",
                HOME_ENTRY,
            ),
            (
                "/editor/project-home-v1.mjs",
                "text/javascript; charset=utf-8",
                PROJECT_HOME_CONTROLLER,
            ),
            (
                "/editor/chaptera-cloud-project-catalog-v1.mjs",
                "text/javascript; charset=utf-8",
                PROJECT_CATALOG_CLIENT,
            ),
        ] {
            let response = routes
                .clone()
                .oneshot(Request::builder().uri(uri).body(Body::empty()).unwrap())
                .await
                .unwrap();
            assert_eq!(response.status(), StatusCode::OK, "{uri}");
            assert_eq!(response.headers()["content-type"], expected_type, "{uri}");
            let response_bytes = to_bytes(response.into_body(), usize::MAX).await.unwrap();
            assert_eq!(response_bytes.as_ref(), bytes, "{uri}");
        }
        let html = std::str::from_utf8(HOME_HTML).unwrap();
        assert!(html.contains("/editor/cloud-project-home-v1.mjs"));
        assert!(!html.contains("x-chaptera-principal-id"));
    }

    #[test]
    fn manifest_covers_every_embedded_editor_asset() {
        let manifest = version_manifest();
        assert_eq!(manifest["asset_count"], ASSETS.len());
        assert_eq!(manifest["files"].as_array().unwrap().len(), ASSETS.len());
    }
}
