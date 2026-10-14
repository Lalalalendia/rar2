use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};
use tokio::{
    fs,
    io::{AsyncRead, AsyncReadExt, AsyncWriteExt},
};

use crate::blob_store::{
    BlobProvider, GrantOperation, ProviderCapabilities, ProviderError, ProviderErrorKind,
    ProviderGrant, ProviderGrantRequest, ProviderObjectMetadata,
};

const GENERATION_PREFIX: &str = "sha256:";
const MAX_LOCATOR_BYTES: usize = 1024;
const COPY_BUFFER_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone)]
pub struct FilesystemBlobProvider {
    root: PathBuf,
}

impl FilesystemBlobProvider {
    pub fn new(root: impl Into<PathBuf>) -> Result<Self, ProviderError> {
        let root = root.into();
        if root.as_os_str().is_empty() {
            return Err(provider_other("filesystem_root_empty"));
        }
        Ok(Self { root })
    }

    fn path_for(&self, object_locator: &str) -> Result<PathBuf, ProviderError> {
        validate_locator(object_locator)?;
        Ok(self.root.join(object_locator))
    }
}

#[async_trait::async_trait]
impl BlobProvider for FilesystemBlobProvider {
    fn capabilities(&self) -> ProviderCapabilities {
        ProviderCapabilities {
            hard_create_only: true,
            hard_exact_or_max_upload_size: true,
            signed_content_type: false,
            strong_head_after_put: true,
        }
    }

    async fn create_immutable(
        &self,
        object_locator: &str,
        expected_byte_len: u64,
        mut input: Box<dyn AsyncRead + Unpin + Send>,
    ) -> Result<ProviderObjectMetadata, ProviderError> {
        let path = self.path_for(object_locator)?;
        let parent = path
            .parent()
            .ok_or_else(|| provider_other("filesystem_parent_missing"))?;
        fs::create_dir_all(parent)
            .await
            .map_err(|_| provider_other("filesystem_mkdir_failed"))?;

        let mut output = match fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .open(&path)
            .await
        {
            Ok(file) => file,
            Err(error) if error.kind() == std::io::ErrorKind::AlreadyExists => {
                return Err(ProviderError::new(
                    ProviderErrorKind::AlreadyExists,
                    "filesystem_object_exists",
                ));
            }
            Err(_) => return Err(provider_other("filesystem_create_failed")),
        };

        let mut hasher = Sha256::new();
        let mut total = 0_u64;
        let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
        let result = async {
            loop {
                let count = input
                    .read(&mut buffer)
                    .await
                    .map_err(|_| provider_other("filesystem_input_read_failed"))?;
                if count == 0 {
                    break;
                }
                total = total
                    .checked_add(
                        u64::try_from(count)
                            .map_err(|_| provider_other("filesystem_length_overflow"))?,
                    )
                    .ok_or_else(|| provider_other("filesystem_length_overflow"))?;
                if total > expected_byte_len {
                    return Err(provider_other("filesystem_input_too_long"));
                }
                hasher.update(&buffer[..count]);
                output
                    .write_all(&buffer[..count])
                    .await
                    .map_err(|_| provider_other("filesystem_write_failed"))?;
            }
            if total != expected_byte_len {
                return Err(provider_other("filesystem_input_too_short"));
            }
            output
                .flush()
                .await
                .map_err(|_| provider_other("filesystem_flush_failed"))?;
            output
                .sync_all()
                .await
                .map_err(|_| provider_other("filesystem_sync_failed"))?;
            Ok::<(), ProviderError>(())
        }
        .await;

        if let Err(error) = result {
            drop(output);
            let _ = fs::remove_file(&path).await;
            return Err(error);
        }
        drop(output);

        let generation = format!("{GENERATION_PREFIX}{:x}", hasher.finalize());
        Ok(ProviderObjectMetadata {
            generation: generation.clone(),
            byte_len: total,
            etag: generation,
        })
    }

    async fn head_exact(
        &self,
        object_locator: &str,
    ) -> Result<Option<ProviderObjectMetadata>, ProviderError> {
        let path = self.path_for(object_locator)?;
        let metadata = match fs::metadata(&path).await {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(None),
            Err(_) => return Err(provider_other("filesystem_metadata_failed")),
        };
        if !metadata.is_file() {
            return Err(provider_other("filesystem_object_not_file"));
        }
        let (byte_len, generation) = hash_file(&path).await?;
        if byte_len != metadata.len() {
            return Err(provider_other("filesystem_object_changed_during_head"));
        }
        Ok(Some(ProviderObjectMetadata {
            generation: generation.clone(),
            byte_len,
            etag: generation,
        }))
    }

    async fn open_read(
        &self,
        object_locator: &str,
        generation: &str,
    ) -> Result<Box<dyn AsyncRead + Unpin + Send>, ProviderError> {
        validate_generation(generation)?;
        let path = self.path_for(object_locator)?;
        let Some(metadata) = self.head_exact(object_locator).await? else {
            return Err(ProviderError::new(
                ProviderErrorKind::NotFound,
                "filesystem_object_not_found",
            ));
        };
        if metadata.generation != generation {
            return Err(ProviderError::new(
                ProviderErrorKind::NotFound,
                "filesystem_generation_mismatch",
            ));
        }
        let file = fs::File::open(&path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ProviderError::new(ProviderErrorKind::NotFound, "filesystem_object_not_found")
            } else {
                provider_other("filesystem_open_failed")
            }
        })?;
        Ok(Box::new(file))
    }

    async fn delete_exact(
        &self,
        object_locator: &str,
        generation: &str,
    ) -> Result<(), ProviderError> {
        validate_generation(generation)?;
        let Some(metadata) = self.head_exact(object_locator).await? else {
            return Err(ProviderError::new(
                ProviderErrorKind::NotFound,
                "filesystem_object_not_found",
            ));
        };
        if metadata.generation != generation {
            return Err(ProviderError::new(
                ProviderErrorKind::NotFound,
                "filesystem_generation_mismatch",
            ));
        }
        let path = self.path_for(object_locator)?;
        fs::remove_file(path).await.map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                ProviderError::new(ProviderErrorKind::NotFound, "filesystem_object_not_found")
            } else {
                provider_other("filesystem_delete_failed")
            }
        })
    }

    async fn issue_grant(
        &self,
        request: &ProviderGrantRequest,
    ) -> Result<ProviderGrant, ProviderError> {
        let _ = self.path_for(&request.object_locator)?;
        match request.operation {
            GrantOperation::UploadCreateOnly | GrantOperation::Download => Err(ProviderError::new(
                ProviderErrorKind::Other,
                "filesystem_direct_grants_unsupported",
            )),
        }
    }
}

async fn hash_file(path: &Path) -> Result<(u64, String), ProviderError> {
    let mut file = fs::File::open(path).await.map_err(|error| {
        if error.kind() == std::io::ErrorKind::NotFound {
            ProviderError::new(ProviderErrorKind::NotFound, "filesystem_object_not_found")
        } else {
            provider_other("filesystem_open_failed")
        }
    })?;
    let mut hasher = Sha256::new();
    let mut total = 0_u64;
    let mut buffer = vec![0_u8; COPY_BUFFER_BYTES];
    loop {
        let count = file
            .read(&mut buffer)
            .await
            .map_err(|_| provider_other("filesystem_read_failed"))?;
        if count == 0 {
            break;
        }
        total = total
            .checked_add(
                u64::try_from(count).map_err(|_| provider_other("filesystem_length_overflow"))?,
            )
            .ok_or_else(|| provider_other("filesystem_length_overflow"))?;
        hasher.update(&buffer[..count]);
    }
    Ok((total, format!("{GENERATION_PREFIX}{:x}", hasher.finalize())))
}

fn validate_generation(generation: &str) -> Result<(), ProviderError> {
    let Some(hex) = generation.strip_prefix(GENERATION_PREFIX) else {
        return Err(provider_other("filesystem_generation_invalid"));
    };
    if hex.len() != 64 || !hex.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return Err(provider_other("filesystem_generation_invalid"));
    }
    Ok(())
}

fn validate_locator(locator: &str) -> Result<(), ProviderError> {
    if locator.is_empty()
        || locator.len() > MAX_LOCATOR_BYTES
        || locator.starts_with('/')
        || locator.ends_with('/')
        || locator.contains('\\')
        || locator.chars().any(char::is_control)
    {
        return Err(provider_other("filesystem_locator_invalid"));
    }
    let mut segments = locator.split('/');
    let namespace = segments
        .next()
        .ok_or_else(|| provider_other("filesystem_locator_invalid"))?;
    if !matches!(
        namespace,
        "quarantine" | "canonical" | "checkpoints" | "derived" | "exports"
    ) {
        return Err(provider_other("filesystem_namespace_unsupported"));
    }
    let rest = segments.collect::<Vec<_>>();
    if rest.len() < 2
        || rest
            .iter()
            .any(|segment| segment.is_empty() || *segment == "." || *segment == "..")
    {
        return Err(provider_other("filesystem_locator_invalid"));
    }
    Ok(())
}

fn provider_other(code: impl Into<String>) -> ProviderError {
    ProviderError::new(ProviderErrorKind::Other, code)
}

#[cfg(test)]
mod tests {
    use std::io::Cursor;

    use super::*;

    fn temp_root(label: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "chaptera-fs-blob-{label}-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[tokio::test]
    async fn filesystem_provider_round_trips_and_deletes_exact_generation() {
        let root = temp_root("roundtrip");
        let provider = FilesystemBlobProvider::new(&root).unwrap();
        let bytes = b"chaptera".to_vec();
        let metadata = provider
            .create_immutable(
                "quarantine/tenant/upload",
                bytes.len() as u64,
                Box::new(Cursor::new(bytes.clone())),
            )
            .await
            .unwrap();

        assert_eq!(metadata.byte_len, bytes.len() as u64);
        let head = provider
            .head_exact("quarantine/tenant/upload")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(head, metadata);

        let mut reader = provider
            .open_read("quarantine/tenant/upload", &metadata.generation)
            .await
            .unwrap();
        let mut actual = Vec::new();
        reader.read_to_end(&mut actual).await.unwrap();
        assert_eq!(actual, bytes);

        provider
            .delete_exact("quarantine/tenant/upload", &metadata.generation)
            .await
            .unwrap();
        assert!(
            provider
                .head_exact("quarantine/tenant/upload")
                .await
                .unwrap()
                .is_none()
        );
        let _ = fs::remove_dir_all(root).await;
    }

    #[tokio::test]
    async fn filesystem_provider_rejects_escape_and_overlong_input() {
        let root = temp_root("reject");
        let provider = FilesystemBlobProvider::new(&root).unwrap();
        assert!(provider.head_exact("quarantine/../escape").await.is_err());
        let error = provider
            .create_immutable(
                "quarantine/tenant/upload",
                3,
                Box::new(Cursor::new(b"four".to_vec())),
            )
            .await
            .unwrap_err();
        assert_eq!(error.code, "filesystem_input_too_long");
        assert!(
            provider
                .head_exact("quarantine/tenant/upload")
                .await
                .unwrap()
                .is_none()
        );
        let _ = fs::remove_dir_all(root).await;
    }
}
