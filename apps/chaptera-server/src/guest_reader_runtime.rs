use std::{error::Error, io, time::Duration};

use crate::{
    blob_runtime::BlobStoreRuntime,
    config::{ChapteraConfig, ResolvedSecrets},
    guest_reader_http::{
        self, GuestReaderHttpConfig, GuestReaderHttpState, SqliteGuestReaderSessionStore,
    },
    guest_reader_worker::IsolatedGuestSceneProducer,
    public_rate_limit::SqlitePublicRateLimitAuthority,
    source_ingress_security::ProductionSourceSecurityScanner,
    upload_admission::SqliteUploadAdmissionAuthority,
};

pub async fn build_guest_reader_router(
    config: &ChapteraConfig,
    secrets: &ResolvedSecrets,
    blob_store: &BlobStoreRuntime,
) -> Result<Option<axum::Router>, Box<dyn Error>> {
    let Some(guest_config) = &config.cloud_reader_guest else {
        return Ok(None);
    };

    let busy_timeout = Duration::from_millis(config.sqlite.busy_timeout_ms);
    let rate_secret = secrets
        .cloud_reader_guest_rate_secret
        .as_ref()
        .ok_or_else(|| {
            io::Error::new(
                io::ErrorKind::InvalidInput,
                "cloud_reader_guest rate subject secret was not resolved",
            )
        })?;
    let guest_rate = SqlitePublicRateLimitAuthority::open(
        &config.sqlite.path,
        config.sqlite.pool_max,
        busy_timeout,
        guest_config.public_rate_limit(),
        rate_secret.expose(),
    )
    .await?;
    let guest_admission = SqliteUploadAdmissionAuthority::open(
        &config.sqlite.path,
        config.sqlite.pool_max,
        busy_timeout,
        guest_config.upload_admission(),
    )
    .await?;
    let guest_sessions = SqliteGuestReaderSessionStore::open(
        &config.sqlite.path,
        config.sqlite.pool_max,
        busy_timeout,
    )
    .await?;
    let guest_scan_config = config.source_validation.materialize();
    let guest_scanner = ProductionSourceSecurityScanner::new(guest_scan_config.clone())?;
    let guest_scene_worker = IsolatedGuestSceneProducer::new_with_fonts(
        guest_scan_config,
        &guest_config.font_resources,
    )?;
    let guest_state = GuestReaderHttpState::new(
        guest_rate,
        guest_admission,
        guest_sessions,
        blob_store.service().clone(),
        guest_scanner,
        guest_scene_worker,
        GuestReaderHttpConfig {
            session_ttl: Duration::from_secs(guest_config.session_ttl_seconds),
            max_file_bytes: guest_config.max_file_bytes,
        },
    )?;
    let guest_cleanup_state = guest_state.clone();
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(Duration::from_secs(30));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
        loop {
            interval.tick().await;
            if let Err(error) = guest_cleanup_state.cleanup_expired_sessions().await {
                eprintln!("chaptera_guest_cleanup {error}");
            }
        }
    });

    Ok(Some(guest_reader_http::router(guest_state)))
}
