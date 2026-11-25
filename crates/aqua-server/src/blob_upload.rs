//! Resumable delivery-cache uploads. The relay only handles ciphertext.

use std::{
    fs::{self, File, OpenOptions},
    io::{Read, Seek, SeekFrom, Write},
    path::{Path as FsPath, PathBuf},
    time::Duration,
};

use aqua_core::file_transfer::{chunk_count, CHUNK_TAG, STREAM_CHUNK};
use aqua_core::{hex_sha256, valid_cid};
use axum::{
    body::{Body, Bytes},
    extract::{Path, Query, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::{
    blobstore::{BlobStore, Shelf},
    fed::Fed,
    quota, AppState, Caller,
};

const LIFETIME: Duration = Duration::from_secs(86_400);
type UploadError = (StatusCode, String);

#[derive(Deserialize)]
pub struct Part {
    size: u64,
    #[serde(default)]
    offset: u64,
    hash: Option<String>,
    kind: Option<String>,
}

#[derive(Serialize)]
pub struct Progress {
    offset: u64,
    complete: bool,
}

fn folder(store: &BlobStore, owner: &str) -> PathBuf {
    store.upload_root().join(hex_sha256(owner.as_bytes()))
}

fn expired(entry: &fs::DirEntry) -> bool {
    entry
        .metadata()
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.elapsed().ok())
        .is_some_and(|age| age >= LIFETIME)
}

fn entries(dir: &FsPath) -> impl Iterator<Item = fs::DirEntry> {
    fs::read_dir(dir).into_iter().flatten().flatten()
}

fn reserved_except(dir: &FsPath, cid: &str) -> u64 {
    entries(dir)
        .filter(|entry| !expired(entry))
        .filter_map(|entry| {
            let name = entry.file_name();
            let name = name.to_str()?.strip_suffix(".size")?;
            if name == cid {
                return None;
            }
            fs::read_to_string(entry.path()).ok()?.parse::<u64>().ok()
        })
        .fold(0, u64::saturating_add)
}

pub(crate) fn reserved(store: &BlobStore, owner: &str) -> u64 {
    reserved_except(&folder(store, owner), "")
}

pub(crate) fn sweep(root: &FsPath) {
    for dir in entries(root) {
        if !dir.file_type().is_ok_and(|t| t.is_dir()) {
            continue;
        }
        for entry in entries(&dir.path()).filter(expired) {
            if entry.file_type().is_ok_and(|t| t.is_file()) {
                let _ = fs::remove_file(entry.path());
            }
        }
        let _ = fs::remove_dir(dir.path());
    }
    for entry in entries(root).filter(expired) {
        if entry.file_type().is_ok_and(|t| t.is_file()) {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn io(err: std::io::Error) -> UploadError {
    tracing::warn!(%err, "resumable blob I/O");
    (
        StatusCode::INTERNAL_SERVER_ERROR,
        "upload storage unavailable".into(),
    )
}

fn bad(message: &str) -> UploadError {
    (StatusCode::BAD_REQUEST, message.to_owned())
}

pub async fn put(
    State(state): State<AppState>,
    Caller(dh): Caller,
    Path(cid): Path<String>,
    Query(part): Query<Part>,
    body: Bytes,
) -> Result<Json<Progress>, UploadError> {
    if !valid_cid(&cid) || cid.bytes().any(|b| b.is_ascii_uppercase()) || part.size == 0 {
        return Err(bad("invalid file reference"));
    }
    let (owner, tier) = quota::caller(&state, &dh).await;
    let limits = quota::quotas(&state);
    let max = limits.of(tier).file_max;
    if part.size > max {
        return Err((
            StatusCode::PAYLOAD_TOO_LARGE,
            format!("quota: file_max {max}"),
        ));
    }
    let limit = quota::files_limit(&limits, &owner, tier);
    let total = part.size + chunk_count(part.size, STREAM_CHUNK) * CHUNK_TAG as u64;
    if !body.is_empty()
        && (part.offset > total
            || body.len() as u64 > total - part.offset
            || part.hash.as_deref() != Some(hex_sha256(&body).as_str()))
    {
        return Err(bad("invalid part offset, length or checksum"));
    }
    tokio::task::spawn_blocking(move || {
        let _upload = quota::lock_upload(&owner);
        let store = &state.blobs;
        let dir = folder(store, &owner);
        let staged = dir.join(format!("{cid}.part"));
        let marker = dir.join(format!("{cid}.size"));
        if store.has_cached(&cid) {
            let _ = fs::remove_file(staged);
            let _ = fs::remove_file(marker);
            return Ok(Json(Progress {
                offset: total,
                complete: true,
            }));
        }
        let (used, generation) = quota::usage_snapshot(&state, &owner);
        if total
            > limit
                .saturating_sub(used)
                .saturating_sub(reserved_except(&dir, &cid))
        {
            return Err((
                StatusCode::INSUFFICIENT_STORAGE,
                format!("quota: files {used} {limit}"),
            ));
        }
        if !staged.exists() {
            store.fit(total);
        }
        fs::create_dir_all(&dir).map_err(io)?;
        if let Ok(previous) = fs::read_to_string(&marker) {
            if previous.parse::<u64>().ok() != Some(total) {
                return Err(bad("upload size changed"));
            }
        }
        let mut file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(&staged)
            .map_err(io)?;
        let offset = file.metadata().map_err(io)?.len();
        if offset > total {
            return Err(bad("upload exceeds declared size"));
        }
        fs::write(&marker, total.to_string()).map_err(io)?;
        let mut next = offset;
        if !body.is_empty() && part.offset == offset {
            file.seek(SeekFrom::Start(offset)).map_err(io)?;
            if let Err(e) = file.write_all(&body).and_then(|()| file.sync_data()) {
                let _ = file.set_len(offset);
                return Err(io(e));
            }
            next += body.len() as u64;
        }
        if next != total {
            return Ok(Json(Progress {
                offset: next,
                complete: false,
            }));
        }
        file.seek(SeekFrom::Start(0)).map_err(io)?;
        let mut digest = Sha256::new();
        let mut buffer = vec![0; 128 * 1024];
        loop {
            let n = file.read(&mut buffer).map_err(io)?;
            if n == 0 {
                break;
            }
            digest.update(&buffer[..n]);
        }
        drop(file);
        if format!("{:x}", digest.finalize()) != cid {
            let _ = fs::remove_file(&staged);
            let _ = fs::remove_file(&marker);
            return Err(bad("upload checksum mismatch"));
        }
        if store.commit_upload(&cid, &staged, &owner).map_err(io)? {
            quota::add(&owner, total, generation);
        }
        if let Some(kind) = part.kind {
            store.set_kind(&cid, Shelf::Cache, &kind);
        }
        let _ = fs::remove_file(staged);
        let _ = fs::remove_file(marker);
        Ok(Json(Progress {
            offset: total,
            complete: true,
        }))
    })
    .await
    .map_err(|_| {
        (
            StatusCode::INTERNAL_SERVER_ERROR,
            "upload interrupted".into(),
        )
    })?
}

pub(crate) fn response(file: File, len: u64) -> Response {
    let chunks =
        futures_util::stream::try_unfold(tokio::fs::File::from_std(file), |mut file| async move {
            let mut buffer = vec![0; 128 * 1024];
            let n = file.read(&mut buffer).await?;
            buffer.truncate(n);
            Ok::<_, std::io::Error>((n != 0).then_some((Bytes::from(buffer), file)))
        });
    (
        [
            (header::CONTENT_TYPE, "application/octet-stream".into()),
            (header::CONTENT_LENGTH, len.to_string()),
        ],
        Body::from_stream(chunks),
    )
        .into_response()
}

struct Temporary(PathBuf);
impl Drop for Temporary {
    fn drop(&mut self) {
        let _ = fs::remove_file(&self.0);
    }
}

pub(crate) async fn federated(store: &BlobStore, fed: &Fed, cid: &str) -> Option<Response> {
    if !valid_cid(cid) {
        return None;
    }
    static FED_DOWNLOADS: tokio::sync::Semaphore = tokio::sync::Semaphore::const_new(4);
    let _permit = FED_DOWNLOADS.acquire().await.ok()?;
    let cached = {
        let store = store.clone();
        let cid = cid.to_owned();
        tokio::task::spawn_blocking(move || store.open_blob(&cid))
            .await
            .ok()
            .flatten()
    };
    if let Some((file, size)) = cached {
        return Some(response(file, size));
    }
    let root = store.upload_root();
    tokio::fs::create_dir_all(&root).await.ok()?;
    for peer in fed.peer_list() {
        let Some(mut input) = fed.blob_response(&peer, cid).await else {
            continue;
        };
        let temp = Temporary(root.join(format!("fed-{}.part", uuid::Uuid::new_v4())));
        let mut out = tokio::fs::File::create(&temp.0).await.ok()?;
        let mut digest = Sha256::new();
        let mut length = 0_u64;
        let max =
            quota::FILE_MAX_CAP + chunk_count(quota::FILE_MAX_CAP, STREAM_CHUNK) * CHUNK_TAG as u64;
        let valid = loop {
            match input.chunk().await {
                Ok(Some(bytes)) if length + bytes.len() as u64 <= max => {
                    length += bytes.len() as u64;
                    digest.update(&bytes);
                    out.write_all(&bytes).await.ok()?;
                }
                Ok(None) => break format!("{:x}", digest.finalize()) == cid,
                _ => break false,
            }
        };
        if !valid {
            continue;
        }
        out.sync_data().await.ok()?;
        drop(out);
        let copy = store.clone();
        let cid = cid.to_owned();
        return tokio::task::spawn_blocking(move || {
            copy.commit_upload(&cid, &temp.0, "").ok()?;
            let (file, len) = copy.open_blob(&cid)?;
            Some(response(file, len))
        })
        .await
        .ok()
        .flatten();
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn continues_at_committed_offset_and_charges_once() {
        let dir = tempfile::tempdir().unwrap();
        let state = AppState::isolated(dir.path().to_owned(), None);
        let body = Bytes::from(vec![42; 128 + CHUNK_TAG]);
        let cid = hex_sha256(&body);
        let send = |offset, data: Bytes, hash| {
            put(
                State(state.clone()),
                Caller([99; 32]),
                Path(cid.clone()),
                Query(Part {
                    size: 128,
                    offset,
                    hash,
                    kind: None,
                }),
                data,
            )
        };
        assert_eq!(send(0, Bytes::new(), None).await.unwrap().0.offset, 0);
        assert_eq!(reserved(&state.blobs, quota::GUESTS), body.len() as u64);
        let first = body.slice(..47);
        assert_eq!(
            send(0, first.clone(), Some("bad".into()))
                .await
                .err()
                .unwrap()
                .0,
            StatusCode::BAD_REQUEST
        );
        assert_eq!(
            send(0, first.clone(), Some(hex_sha256(&first)))
                .await
                .unwrap()
                .0
                .offset,
            47
        );
        assert_eq!(
            send(0, first.clone(), Some(hex_sha256(&first)))
                .await
                .unwrap()
                .0
                .offset,
            47
        );
        assert_eq!(send(0, Bytes::new(), None).await.unwrap().0.offset, 47);
        let last = body.slice(47..);
        assert!(
            send(47, last.clone(), Some(hex_sha256(&last)))
                .await
                .unwrap()
                .0
                .complete
        );
        assert!(send(0, Bytes::new(), None).await.unwrap().0.complete);
        assert_eq!(reserved(&state.blobs, quota::GUESTS), 0);
        assert_eq!(state.blobs.get(&cid).unwrap(), body.as_ref());
        assert_eq!(
            state.blobs.usage_by_owner().get(quota::GUESTS),
            Some(&(body.len() as u64))
        );
    }
}
