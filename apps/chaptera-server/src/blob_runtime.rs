use std::{fmt::Write as _, path::Path, sync::Arc, time::Duration};

use aws_config::BehaviorVersion;
use rand::{RngCore, rngs::OsRng};

use crate::{
    blob_store::{BlobIdGenerator, BlobProvider, BlobStoreError, BlobStoreService},
    config::ChapteraConfig,
    filesystem_blob_provider::FilesystemBlobProvider,
    s3_blob_provider::S3BlobProvider,
    sqlite_blob_metadata::SqliteBlobBindingRepository,
};

const RANDOM_ID_BYTES: usize = 16;

#[derive(Debug, Clone, Copy, Default)]
pub struct SystemBlobIdGenerator;

impl BlobIdGenerator for SystemBlobIdGenerator {
    fn next_physical_blob_id(&self) -> Result<String, BlobStoreError> {
        random_id("blob")
    }

    fn next_binding_id(&self) -> Result<String, BlobStoreError> {
        random_id("binding")
    }
}

#[derive(Clone)]
pub struct BlobStoreRuntime {
    service: BlobStoreService,
    metadata: SqliteBlobBindingRepository,
}

impl BlobStoreRuntime {
    pub async fn open(config: &ChapteraConfig) -> Result<Self, BlobStoreError> {
        let metadata = SqliteBlobBindingRepository::open(
            &config.sqlite.path,
            config.sqlite.pool_max,
            Duration::from_millis(config.sqlite.busy_timeout_ms),
        )
        .await?;

        let provider: Arc<dyn BlobProvider> = match config.storage.provider.as_str() {
            "s3-compatible" => {
                let shared_config = aws_config::load_defaults(BehaviorVersion::latest()).await;
                let client = aws_sdk_s3::Client::new(&shared_config);
                let provider = S3BlobProvider::new(
                    client,
                    config.storage.quarantine_namespace.clone(),
                    config.storage.private_namespace.clone(),
                    config.storage.expected_bucket_owner.clone(),
                )
                .map_err(|error| {
                    BlobStoreError::new(
                        "blob_provider_config_invalid",
                        bounded_message(&error.code),
                    )
                })?;
                Arc::new(provider)
            }
            "filesystem" => {
                let sqlite_parent = config
                    .sqlite
                    .path
                    .parent()
                    .filter(|path| !path.as_os_str().is_empty())
                    .unwrap_or_else(|| Path::new("."));
                let provider = FilesystemBlobProvider::new(sqlite_parent.join("blob-store"))
                    .map_err(|error| {
                        BlobStoreError::new(
                            "blob_provider_config_invalid",
                            bounded_message(&error.code),
                        )
                    })?;
                Arc::new(provider)
            }
            provider => {
                return Err(BlobStoreError::new(
                    "storage_provider_unsupported",
                    format!("configured storage provider {provider:?} is unsupported"),
                ));
            }
        };

        let service = BlobStoreService::new(
            provider,
            Arc::new(metadata.clone()),
            Arc::new(SystemBlobIdGenerator),
        );

        Ok(Self { service, metadata })
    }

    pub fn service(&self) -> &BlobStoreService {
        &self.service
    }

    pub fn metadata(&self) -> &SqliteBlobBindingRepository {
        &self.metadata
    }

    pub async fn close(&self) {
        self.metadata.close().await;
    }
}

fn random_id(prefix: &'static str) -> Result<String, BlobStoreError> {
    let mut bytes = [0_u8; RANDOM_ID_BYTES];
    OsRng.try_fill_bytes(&mut bytes).map_err(|error| {
        BlobStoreError::new("blob_id_random_failed", bounded_message(&error.to_string()))
    })?;

    let mut out = String::with_capacity(prefix.len() + 1 + RANDOM_ID_BYTES * 2);
    out.push_str(prefix);
    out.push(':');
    for byte in bytes {
        write!(&mut out, "{byte:02x}")
            .expect("writing random blob identifier into String cannot fail");
    }
    Ok(out)
}

fn bounded_message(message: &str) -> String {
    message.chars().take(512).collect()
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeSet;

    use super::*;

    #[test]
    fn system_ids_are_bounded_opaque_and_do_not_cross_identity_kinds() {
        let ids = SystemBlobIdGenerator;
        let mut physical = BTreeSet::new();
        let mut bindings = BTreeSet::new();

        for _ in 0..128 {
            let physical_id = ids.next_physical_blob_id().unwrap();
            let binding_id = ids.next_binding_id().unwrap();

            assert!(physical_id.starts_with("blob:"));
            assert!(binding_id.starts_with("binding:"));
            assert_eq!(physical_id.len(), "blob:".len() + RANDOM_ID_BYTES * 2);
            assert_eq!(binding_id.len(), "binding:".len() + RANDOM_ID_BYTES * 2);
            assert!(
                physical_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b':')
            );
            assert!(
                binding_id
                    .bytes()
                    .all(|byte| byte.is_ascii_alphanumeric() || byte == b':')
            );
            assert!(physical.insert(physical_id));
            assert!(bindings.insert(binding_id));
        }
    }
}
