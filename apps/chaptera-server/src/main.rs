use std::{error::Error, io, process::ExitCode, time::Duration};

use chaptera_server::{
    auth_runtime::AuthRuntime,
    authz_runtime::SqliteAuthzAuthority,
    blob_runtime::BlobStoreRuntime,
    cli::{Cli, Command},
    config::{ChapteraConfig, EnvironmentMode, SecretResolver},
    doctor,
    edge::EdgePolicy,
    guest_reader_http::{
        self, GuestReaderHttpConfig, GuestReaderHttpState, SqliteGuestReaderSessionStore,
    },
    guest_reader_worker::{self, IsolatedGuestSceneProducer},
    install,
    job_queue::SqliteJobQueue,
    jobs::UnconfiguredWorkerRuntime,
    jobs_runtime::JobsRuntime,
    migrate,
    migration_editable_route::{
        self, IsolatedMigrationEditableRouteProducer, MigrationEditableRouteHttpState,
    },
    product_api_http::{self, ProductApiHttpState},
    product_export_http::{self, ProductExportHttpState},
    project_persistence_sqlite::SqliteProjectPersistence,
    public_rate_limit::SqlitePublicRateLimitAuthority,
    revision_materializer::BlobStoreExactSourceLoader,
    runtime_readiness::{ports_with_configured_serve, ports_with_revision_stream},
    schema_migration::SqliteMigrationRuntime,
    serve,
    source_authority::SqliteDocumentSourceAuthority,
    source_baseline,
    source_baseline::IsolatedSourceBaselineProducer,
    source_ingress_http::{self, SourceIngressHttpState},
    source_ingress_security::ProductionSourceSecurityScanner,
    source_ingress_sqlite::SqliteSourceIngressRepository,
    source_validation_job::SourceValidationJobQueue,
    sqlite_store::SqliteRevisionStore,
    state::{AppState, RuntimePorts},
    untrusted_pub_worker,
    upload_admission::SqliteUploadAdmissionAuthority,
    worker,
    worker_runtime::ConfiguredWorkerRuntime,
    workspace_context::SqliteWorkspaceContextResolver,
};
use clap::Parser;

fn main() -> ExitCode {
    let cli = Cli::parse();

    if let Command::UntrustedPubInspect {
        max_file_bytes,
        max_cfb_entries,
        max_declared_stream_bytes,
    } = &cli.command
    {
        return match untrusted_pub_worker::run_inspect(
            *max_file_bytes,
            *max_cfb_entries,
            *max_declared_stream_bytes,
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("chaptera: {error}");
                ExitCode::FAILURE
            }
        };
    }

    if let Command::SourceBaseline {
        document_id,
        expected_sha256,
        expected_byte_len,
    } = &cli.command
    {
        return match source_baseline::run_source_baseline_worker(
            document_id,
            expected_sha256,
            *expected_byte_len,
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("chaptera: {error}");
                ExitCode::FAILURE
            }
        };
    }

    if let Command::MigrationEditableRoutes {
        document_id,
        expected_sha256,
        expected_byte_len,
    } = &cli.command
    {
        return match migration_editable_route::run_migration_editable_routes_worker(
            document_id,
            expected_sha256,
            *expected_byte_len,
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("chaptera: {error}");
                ExitCode::FAILURE
            }
        };
    }

    if let Command::GuestReaderScene {
        session_id,
        expected_sha256,
        expected_byte_len,
        font_registry,
    } = &cli.command
    {
        return match guest_reader_worker::run_guest_scene_worker(
            session_id,
            expected_sha256,
            *expected_byte_len,
            font_registry.as_deref(),
        ) {
            Ok(()) => ExitCode::SUCCESS,
            Err(error) => {
                eprintln!("chaptera: {error}");
                ExitCode::FAILURE
            }
        };
    }

    let runtime = match tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
    {
        Ok(runtime) => runtime,
        Err(error) => {
            eprintln!("chaptera: failed to initialize runtime: {error}");
            return ExitCode::FAILURE;
        }
    };

    match runtime.block_on(run(cli)) {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("chaptera: {error}");
            ExitCode::FAILURE
        }
    }
}

async fn run(cli: Cli) -> Result<(), Box<dyn Error>> {
    let explicit_config = cli
        .config
        .as_deref()
        .map(ChapteraConfig::load)
        .transpose()?;

    match cli.command {
        Command::Install { root } => {
            install::run(cli.config.as_deref(), root.as_deref())?;
        }
        Command::Serve => {
            let config = match explicit_config.as_ref() {
                Some(config) => config.clone(),
                None => ChapteraConfig::development_from_env()?,
            };
            let secrets = config.resolve_required_secrets(&SecretResolver::from_process())?;

            let edge_policy = EdgePolicy::from_config(&config)?;
            if explicit_config.is_some() {
                let revision_stream = SqliteRevisionStore::open(
                    &config.sqlite.path,
                    config.sqlite.pool_max,
                    Duration::from_millis(config.sqlite.busy_timeout_ms),
                )
                .await?;
                if config.auth.is_some() {
                    let auth_runtime = AuthRuntime::open(&config, &secrets).await?;
                    let auth_http = auth_runtime.http_state();
                    let busy_timeout = Duration::from_millis(config.sqlite.busy_timeout_ms);
                    let authz = SqliteAuthzAuthority::open(
                        &config.sqlite.path,
                        config.sqlite.pool_max,
                        busy_timeout,
                    )
                    .await?;
                    let jobs = JobsRuntime::open_with_authz(
                        &config.sqlite.path,
                        config.sqlite.pool_max,
                        busy_timeout,
                        authz.clone(),
                    )
                    .await?;
                    let blob_store = BlobStoreRuntime::open(&config).await?;
                    let mut product_router = if let Some(source_config) = &config.source_ingress {
                        let workspace = SqliteWorkspaceContextResolver::open(
                            &config.sqlite.path,
                            config.sqlite.pool_max,
                            busy_timeout,
                        )
                        .await?;
                        let admission = SqliteUploadAdmissionAuthority::open(
                            &config.sqlite.path,
                            config.sqlite.pool_max,
                            busy_timeout,
                            config.upload_admission.materialize(),
                        )
                        .await?;
                        let source_repo = SqliteSourceIngressRepository::open(
                            &config.sqlite.path,
                            config.sqlite.pool_max,
                            busy_timeout,
                        )
                        .await?;
                        let validation_jobs = SourceValidationJobQueue::new(
                            SqliteJobQueue::open(
                                &config.sqlite.path,
                                config.sqlite.pool_max,
                                busy_timeout,
                            )
                            .await?,
                        );
                        let baseline = IsolatedSourceBaselineProducer::new(
                            source_ingress_http::baseline_config(source_config),
                            blob_store.service().clone(),
                        )?;
                        let projects = SqliteProjectPersistence::open(
                            &config.sqlite.path,
                            config.sqlite.pool_max,
                            busy_timeout,
                        )
                        .await?;
                        let source_state = SourceIngressHttpState::new(
                            auth_http.clone(),
                            workspace,
                            admission,
                            source_repo,
                            blob_store.service().clone(),
                            validation_jobs,
                            baseline,
                            projects,
                            source_ingress_http::http_config(source_config),
                        )?;
                        let source_authority = SqliteDocumentSourceAuthority::open(
                            &config.sqlite.path,
                            config.sqlite.pool_max,
                            busy_timeout,
                        )
                        .await?;
                        let product_state = ProductApiHttpState::new(
                            auth_http.clone(),
                            source_authority.clone(),
                            authz.clone(),
                            revision_stream.clone(),
                            BlobStoreExactSourceLoader::new(blob_store.service().clone()),
                        )?;
                        let export_state = ProductExportHttpState::new(
                            auth_http.clone(),
                            source_authority.clone(),
                            revision_stream.clone(),
                            jobs.clone(),
                            blob_store.service().clone(),
                        );
                        let migration_route_producer =
                            IsolatedMigrationEditableRouteProducer::new(
                                source_ingress_http::baseline_config(source_config),
                                blob_store.service().clone(),
                            )?;
                        let migration_route_state = MigrationEditableRouteHttpState::new(
                            auth_http.clone(),
                            source_authority,
                            authz.clone(),
                            migration_route_producer,
                        );
                        Some(
                            source_ingress_http::router(source_state)
                                .merge(product_api_http::router(product_state))
                                .merge(product_export_http::router(export_state))
                                .merge(migration_editable_route::router(migration_route_state)),
                        )
                    } else {
                        None
                    };

                    if let Some(guest_config) = &config.cloud_reader_guest {
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
                        let guest_scanner =
                            ProductionSourceSecurityScanner::new(guest_scan_config.clone())?;
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
                            interval
                                .set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                            loop {
                                interval.tick().await;
                                if let Err(error) =
                                    guest_cleanup_state.cleanup_expired_sessions().await
                                {
                                    eprintln!("chaptera_guest_cleanup {error}");
                                }
                            }
                        });
                        let guest_router = guest_reader_http::router(guest_state);
                        product_router = Some(match product_router {
                            Some(router) => router.merge(guest_router),
                            None => guest_router,
                        });
                    }
                    drop(secrets);

                    let assembled = ports_with_configured_serve(
                        revision_stream,
                        auth_runtime,
                        authz,
                        jobs,
                        blob_store,
                    );
                    let state = AppState::new(assembled.ports);
                    serve::run_with_auth_local_product(
                        config.runtime_config(),
                        edge_policy,
                        state,
                        Some(auth_http),
                        !matches!(config.environment, EnvironmentMode::Prod),
                        product_router,
                    )
                    .await?;
                } else {
                    drop(secrets);
                    let assembled = ports_with_revision_stream(revision_stream);
                    let state = AppState::new(assembled.ports);
                    serve::run_with_auth_local(
                        config.runtime_config(),
                        edge_policy,
                        state,
                        None,
                        !matches!(config.environment, EnvironmentMode::Prod),
                    )
                    .await?;
                }
            } else {
                drop(secrets);
                let state = AppState::new(RuntimePorts::unconfigured());
                serve::run_with_auth_local(config.runtime_config(), edge_policy, state, None, true)
                    .await?;
            }
        }
        Command::Worker => {
            if let Some(config) = explicit_config.as_ref() {
                // The background worker consumes durable AuthZ grants, not the
                // browser OIDC client secret. Do not resolve web-auth secrets
                // here merely for symmetry with serve; BlobStore credentials
                // are owned by the AWS SDK provider credential chain.
                let runtime = ConfiguredWorkerRuntime::new(config.clone());
                worker::run(&runtime).await?;
            } else {
                worker::run(&UnconfiguredWorkerRuntime).await?;
            }
        }
        Command::Migrate { action } => {
            if let Some(config) = explicit_config.as_ref() {
                let runtime = SqliteMigrationRuntime::new(
                    &config.sqlite.path,
                    Duration::from_millis(config.sqlite.busy_timeout_ms),
                )?;
                migrate::run(action, &runtime).await?;
            } else {
                let runtime = SqliteMigrationRuntime::from_env()?;
                migrate::run(action, &runtime).await?;
            }
        }
        Command::Doctor => {
            if let Some(config) = explicit_config.as_ref() {
                let secrets = config.resolve_required_secrets(&SecretResolver::from_process())?;
                drop(secrets);
            }
            doctor::run(&AppState::new(RuntimePorts::unconfigured()))?;
        }
        Command::UntrustedPubInspect { .. } => unreachable!(
            "untrusted-pub-inspect is dispatched synchronously before Tokio runtime creation"
        ),
        Command::SourceBaseline { .. } => unreachable!(
            "source-baseline is dispatched synchronously before Tokio runtime creation"
        ),
        Command::MigrationEditableRoutes { .. } => unreachable!(
            "migration-editable-routes is dispatched synchronously before Tokio runtime creation"
        ),
        Command::GuestReaderScene { .. } => unreachable!(
            "guest-reader-scene is dispatched synchronously before Tokio runtime creation"
        ),
    }

    Ok(())
}
