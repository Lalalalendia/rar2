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

const INDEX_HTML: &[u8] = include_bytes!("../../cloud-reader/index.html");
const READER_CSS: &[u8] = include_bytes!("../../cloud-reader/reader.css");
const READER_APP: &[u8] = include_bytes!("../../cloud-reader/reader-app.mjs");
const READER_MODEL: &[u8] = include_bytes!("../../cloud-reader/reader-model.mjs");
const RENDER_V1: &[u8] = include_bytes!("../../cloud-reader/render-v1.mjs");
const OBSERVABILITY_V1: &[u8] = include_bytes!("../../web/observability-v1.mjs");

const CONTENT_SECURITY_POLICY: &str = "default-src 'none'; script-src 'self'; style-src 'self'; img-src 'self' data:; font-src data:; connect-src 'self'; base-uri 'none'; object-src 'none'; frame-ancestors 'none'; form-action 'none'";
const PERMISSIONS_POLICY: &str = "camera=(), microphone=(), geolocation=()";

struct EmbeddedAsset {
    name: &'static str,
    content_type: &'static str,
    bytes: &'static [u8],
}

const ASSETS: [EmbeddedAsset; 6] = [
    EmbeddedAsset {
        name: "index.html",
        content_type: "text/html; charset=utf-8",
        bytes: INDEX_HTML,
    },
    EmbeddedAsset {
        name: "reader.css",
        content_type: "text/css; charset=utf-8",
        bytes: READER_CSS,
    },
    EmbeddedAsset {
        name: "reader-app.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: READER_APP,
    },
    EmbeddedAsset {
        name: "reader-model.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: READER_MODEL,
    },
    EmbeddedAsset {
        name: "render-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: RENDER_V1,
    },
    EmbeddedAsset {
        name: "observability-v1.mjs",
        content_type: "text/javascript; charset=utf-8",
        bytes: OBSERVABILITY_V1,
    },
];

pub fn router<S>() -> Router<S>
where
    S: Clone + Send + Sync + 'static,
{
    Router::new()
        .route("/", get(index))
        .route("/index.html", get(index))
        .route("/reader.css", get(reader_css))
        .route("/reader-app.mjs", get(reader_app))
        .route("/reader-model.mjs", get(reader_model))
        .route("/render-v1.mjs", get(render_v1))
        .route("/observability-v1.mjs", get(observability_v1))
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

    json!({
        "embedded": true,
        "asset_count": ASSETS.len(),
        "files": files,
    })
}

async fn index() -> Response {
    asset_response(&ASSETS[0])
}

async fn reader_css() -> Response {
    asset_response(&ASSETS[1])
}

async fn reader_app() -> Response {
    asset_response(&ASSETS[2])
}

async fn reader_model() -> Response {
    asset_response(&ASSETS[3])
}

async fn render_v1() -> Response {
    asset_response(&ASSETS[4])
}

async fn observability_v1() -> Response {
    asset_response(&ASSETS[5])
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
    use axum::{
        body::{Body, to_bytes},
        http::Request,
    };
    use tower::ServiceExt;

    use super::*;

    #[tokio::test]
    async fn serves_reader_from_embedded_bytes() {
        let response = router::<()>()
            .oneshot(Request::builder().uri("/").body(Body::empty()).unwrap())
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "text/html; charset=utf-8"
        );
        assert!(response.headers().contains_key("content-security-policy"));

        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), INDEX_HTML);
    }

    #[tokio::test]
    async fn serves_module_with_javascript_content_type() {
        let response = router::<()>()
            .oneshot(
                Request::builder()
                    .uri("/reader-app.mjs")
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();

        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(
            response.headers().get(CONTENT_TYPE).unwrap(),
            "text/javascript; charset=utf-8"
        );
        let body = to_bytes(response.into_body(), usize::MAX).await.unwrap();
        assert_eq!(body.as_ref(), READER_APP);
    }

    #[test]
    fn version_manifest_covers_all_embedded_assets() {
        let manifest = version_manifest();
        assert_eq!(manifest["embedded"], true);
        assert_eq!(manifest["asset_count"], 6);
        let files = manifest["files"].as_array().unwrap();
        assert_eq!(files.len(), 6);
        assert!(files.iter().all(|file| {
            file["sha256"]
                .as_str()
                .is_some_and(|value| value.len() == 64)
        }));
    }
}
