//! In-memory cache: one loaded workspace per manifest directory, plus per-item metadata.

use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fs;
use std::io::ErrorKind;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Instant;

use incan_core::interop::RustItemMetadata;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};

use crate::cache_resolve::{crate_name_for_path, dependency_manifest_dir_for_crate};
use crate::cache_timing::{CallTrace, log_timing_stage, rust_inspect_timing_enabled};
use crate::error::RustMetadataError;
use crate::extractor::extract_rust_item;
use crate::loader::RustWorkspace;

const INSPECTOR_VERSION: &str = env!("CARGO_PKG_VERSION");

/// Cache for [`RustWorkspace`] instances and extracted [`RustItemMetadata`].
///
/// The workspace is loaded at most once per canonical manifest directory; item metadata is stored per `(workspace_root,
/// canonical_path)` and reused without re-querying salsa.
///
/// The entire cache is protected by one mutex so `RustWorkspace` (which is not `Sync` because of the retained `Vfs`)
/// never has to live inside `Arc` for cross-thread sharing.
pub struct RustMetadataCache {
    inner: Mutex<CacheInner>,
}

#[derive(Default)]
struct CacheInner {
    workspaces: HashMap<(PathBuf, bool), RustWorkspace>,
    items: HashMap<(PathBuf, String), Arc<RustItemMetadata>>,
    disk_cache_state: HashMap<PathBuf, DiskCacheState>,
}

#[derive(Default)]
struct DiskCacheState {
    loaded: bool,
    workspace_fingerprint: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
struct DiskCacheEnvelope {
    cache_format: u32,
    #[serde(alias = "incan_version")]
    inspector_version: String,
    workspace_fingerprint: String,
    items: HashMap<String, RustItemMetadata>,
}

// Bump when extracted metadata semantics change in a way that makes previously persisted items unsafe to reuse.
const DISK_CACHE_FORMAT: u32 = 4;
const DISK_CACHE_FILE: &str = ".incan_rust_inspect_cache.json";
// Backward-compatibility read path for caches written before the crate/module rename.
const LEGACY_DISK_CACHE_FILE: &str = ".incan_rust_metadata_cache.json";

/// Canonical on-disk cache path for a generated lock workspace.
fn disk_cache_path(root: &Path) -> PathBuf {
    root.join(DISK_CACHE_FILE)
}

/// Legacy on-disk cache path kept for backward-compatible reads.
fn legacy_disk_cache_path(root: &Path) -> PathBuf {
    root.join(LEGACY_DISK_CACHE_FILE)
}

/// Hash lock-workspace inputs so stale cache files can be ignored cheaply.
fn workspace_fingerprint(root: &Path) -> Result<String, RustMetadataError> {
    let mut hasher = Sha256::new();
    hasher.update(format!("cache_format:{DISK_CACHE_FORMAT}\n").as_bytes());
    hasher.update(format!("inspector_version:{INSPECTOR_VERSION}\n").as_bytes());
    hasher.update(fs::read(root.join("Cargo.toml"))?);
    match fs::read(root.join("Cargo.lock")) {
        Ok(lock) => hasher.update(lock),
        Err(err) if err.kind() == ErrorKind::NotFound => {}
        Err(err) => return Err(err.into()),
    }
    Ok(hex::encode(hasher.finalize()))
}

fn read_json_cache(path: &Path) -> Result<Option<DiskCacheEnvelope>, RustMetadataError> {
    let payload = match fs::read_to_string(path) {
        Ok(payload) => payload,
        Err(err) if err.kind() == ErrorKind::NotFound => return Ok(None),
        Err(err) => return Err(err.into()),
    };
    match serde_json::from_str::<DiskCacheEnvelope>(&payload) {
        Ok(envelope) => Ok(Some(envelope)),
        Err(err) => {
            tracing::warn!(
                path = %path.display(),
                error = %err,
                "ignoring unreadable rust-inspect disk cache (treated as cache miss)"
            );
            if rust_inspect_timing_enabled() {
                eprintln!(
                    "[rust-inspect-timing] disk_cache.parse_error path={} err={err}",
                    path.display()
                );
            }
            Ok(None)
        }
    }
}

/// Load the current disk cache file, then transparently fall back to the legacy filename.
fn read_disk_cache(root: &Path) -> Result<Option<DiskCacheEnvelope>, RustMetadataError> {
    let cache_path = disk_cache_path(root);
    if let Some(envelope) = read_json_cache(&cache_path)? {
        return Ok(Some(envelope));
    }
    read_json_cache(&legacy_disk_cache_path(root))
}

/// Atomically write one cache envelope to disk.
fn write_json_cache(path: &Path, envelope: &DiskCacheEnvelope) -> Result<(), RustMetadataError> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let tmp_path = path.with_extension("tmp");
    let payload = serde_json::to_vec_pretty(envelope).map_err(|err| RustMetadataError::LoadWorkspace {
        path: path.to_path_buf(),
        message: format!("failed to serialize rust-inspect disk cache: {err}"),
    })?;
    fs::write(&tmp_path, payload)?;
    fs::rename(tmp_path, path)?;
    Ok(())
}

/// Persist the current workspace cache snapshot to disk.
fn write_disk_cache(root: &Path, envelope: &DiskCacheEnvelope) -> Result<(), RustMetadataError> {
    let cache_path = disk_cache_path(root);
    write_json_cache(&cache_path, envelope)
}

/// Load valid disk-cache items into memory for one workspace.
fn load_disk_cache_into_memory(inner: &mut CacheInner, root: &Path) -> Result<Option<String>, RustMetadataError> {
    let fingerprint = workspace_fingerprint(root)?;
    let Some(envelope) = read_disk_cache(root)? else {
        return Ok(Some(fingerprint));
    };
    if envelope.cache_format != DISK_CACHE_FORMAT
        || envelope.inspector_version != INSPECTOR_VERSION
        || envelope.workspace_fingerprint != fingerprint
    {
        return Ok(Some(fingerprint));
    }
    for (canonical_path, metadata) in envelope.items {
        inner
            .items
            .insert((root.to_path_buf(), canonical_path), Arc::new(metadata));
    }
    Ok(Some(fingerprint))
}

/// Ensure the workspace-local disk cache has been loaded once for this process.
fn ensure_disk_cache_loaded(inner: &mut CacheInner, root: &Path) -> Result<(), RustMetadataError> {
    if inner.disk_cache_state.get(root).is_some_and(|state| state.loaded) {
        return Ok(());
    }
    let fingerprint = load_disk_cache_into_memory(inner, root)?;
    let state = inner.disk_cache_state.entry(root.to_path_buf()).or_default();
    state.workspace_fingerprint = fingerprint;
    state.loaded = true;
    Ok(())
}

/// Persist one extracted/canonicalized item into the workspace-local disk cache snapshot.
fn persist_item_to_disk_cache(
    inner: &CacheInner,
    root: &Path,
    metadata: &RustItemMetadata,
) -> Result<(), RustMetadataError> {
    let fingerprint = inner
        .disk_cache_state
        .get(root)
        .and_then(|state| state.workspace_fingerprint.clone())
        .unwrap_or(workspace_fingerprint(root)?);
    let mut items = HashMap::new();
    for ((item_root, canonical_path), cached) in &inner.items {
        if item_root == root {
            items.insert(canonical_path.clone(), (*cached.as_ref()).clone());
        }
    }
    items.insert(metadata.canonical_path.clone(), metadata.clone());
    let envelope = DiskCacheEnvelope {
        cache_format: DISK_CACHE_FORMAT,
        inspector_version: INSPECTOR_VERSION.to_string(),
        workspace_fingerprint: fingerprint,
        items,
    };
    write_disk_cache(root, &envelope)
}

#[derive(Debug, Clone)]
pub struct CacheLookupHit {
    pub metadata: Arc<RustItemMetadata>,
    pub alias_used: bool,
}

/// Generate canonical-path aliases that account for Rust/Cargo naming and std/core/alloc spellings.
fn canonical_path_aliases(canonical_path: &str) -> Vec<String> {
    let mut aliases = Vec::new();

    let stripped_raw_idents = canonical_path
        .split("::")
        .map(|segment| segment.strip_prefix("r#").unwrap_or(segment))
        .collect::<Vec<_>>()
        .join("::");
    if stripped_raw_idents != canonical_path {
        aliases.push(stripped_raw_idents);
    }

    if let Some((crate_name, rest)) = canonical_path.split_once("::") {
        if crate_name.contains('_') {
            aliases.push(format!("{}::{rest}", crate_name.replace('_', "-")));
        }
        if crate_name.contains('-') {
            aliases.push(format!("{}::{rest}", crate_name.replace('-', "_")));
        }
    }

    for (prefix, replacement) in [
        ("std::option::", "core::option::"),
        ("std::result::", "core::result::"),
        ("std::string::", "alloc::string::"),
        ("std::vec::", "alloc::vec::"),
        ("std::boxed::", "alloc::boxed::"),
    ] {
        if let Some(rest) = canonical_path.strip_prefix(prefix) {
            aliases.push(format!("{replacement}{rest}"));
        }
    }

    if canonical_path == "std::collections::HashMap" {
        aliases.push("hashbrown::HashMap".to_string());
    } else if let Some(rest) = canonical_path.strip_prefix("std::collections::HashMap::") {
        aliases.push(format!("hashbrown::HashMap::{rest}"));
    }

    aliases
}

/// Build lookup candidates in preferred order for extraction and cache hits.
fn canonical_path_candidates(canonical_path: &str) -> Vec<String> {
    let aliases = canonical_path_aliases(canonical_path);
    if canonical_path.starts_with("std::") && !aliases.is_empty() {
        aliases
            .into_iter()
            .chain(std::iter::once(canonical_path.to_string()))
            .collect()
    } else {
        std::iter::once(canonical_path.to_string()).chain(aliases).collect()
    }
}

/// Attempt extraction through primary workspace, out-dirs workspace, then resolved dependency workspace.
fn extract_in_workspace_set(
    inner: &mut CacheInner,
    root: &Path,
    canonical_path: &str,
    registry_src_roots: Option<&[PathBuf]>,
    progress: &(dyn Fn(String) + Sync),
    timing_enabled: bool,
) -> Result<RustItemMetadata, RustMetadataError> {
    let mut deferred_load_error = None;

    match inner.workspaces.entry((root.to_path_buf(), false)) {
        Entry::Occupied(o) => {
            let started = Instant::now();
            match extract_rust_item(o.into_mut(), canonical_path) {
                Ok(meta) => {
                    log_timing_stage(
                        timing_enabled,
                        root,
                        canonical_path,
                        "extract.workspace.primary",
                        started.elapsed(),
                        "workspace_hit=true out_dirs=false",
                    );
                    return Ok(meta);
                }
                Err(RustMetadataError::CrateNotFound(_)) | Err(RustMetadataError::PathNotResolved(_)) => {}
                Err(err) => return Err(err),
            }
            log_timing_stage(
                timing_enabled,
                root,
                canonical_path,
                "extract.workspace.primary",
                started.elapsed(),
                "workspace_hit=true out_dirs=false status=miss",
            );
        }
        Entry::Vacant(v) => {
            let load_started = Instant::now();
            match RustWorkspace::load(root, progress) {
                Ok(workspace) => {
                    log_timing_stage(
                        timing_enabled,
                        root,
                        canonical_path,
                        "workspace.load.primary",
                        load_started.elapsed(),
                        "out_dirs=false status=ok",
                    );
                    let extract_started = Instant::now();
                    match extract_rust_item(v.insert(workspace), canonical_path) {
                        Ok(meta) => {
                            log_timing_stage(
                                timing_enabled,
                                root,
                                canonical_path,
                                "extract.workspace.primary",
                                extract_started.elapsed(),
                                "workspace_hit=false out_dirs=false",
                            );
                            return Ok(meta);
                        }
                        Err(RustMetadataError::CrateNotFound(_)) | Err(RustMetadataError::PathNotResolved(_)) => {}
                        Err(err) => return Err(err),
                    }
                    log_timing_stage(
                        timing_enabled,
                        root,
                        canonical_path,
                        "extract.workspace.primary",
                        extract_started.elapsed(),
                        "workspace_hit=false out_dirs=false status=miss",
                    );
                }
                Err(err) => {
                    log_timing_stage(
                        timing_enabled,
                        root,
                        canonical_path,
                        "workspace.load.primary",
                        load_started.elapsed(),
                        "out_dirs=false status=error",
                    );
                    deferred_load_error = Some(err);
                }
            }
        }
    }

    match inner.workspaces.entry((root.to_path_buf(), true)) {
        Entry::Occupied(o) => {
            let started = Instant::now();
            match extract_rust_item(o.into_mut(), canonical_path) {
                Ok(meta) => {
                    log_timing_stage(
                        timing_enabled,
                        root,
                        canonical_path,
                        "extract.workspace.out_dirs",
                        started.elapsed(),
                        "workspace_hit=true out_dirs=true",
                    );
                    return Ok(meta);
                }
                Err(RustMetadataError::CrateNotFound(_)) | Err(RustMetadataError::PathNotResolved(_)) => {}
                Err(err) => return Err(err),
            }
            log_timing_stage(
                timing_enabled,
                root,
                canonical_path,
                "extract.workspace.out_dirs",
                started.elapsed(),
                "workspace_hit=true out_dirs=true status=miss",
            );
        }
        Entry::Vacant(v) => {
            let load_started = Instant::now();
            match RustWorkspace::load_with_options(root, progress, true) {
                Ok(workspace) => {
                    log_timing_stage(
                        timing_enabled,
                        root,
                        canonical_path,
                        "workspace.load.out_dirs",
                        load_started.elapsed(),
                        "out_dirs=true status=ok",
                    );
                    let extract_started = Instant::now();
                    match extract_rust_item(v.insert(workspace), canonical_path) {
                        Ok(meta) => {
                            log_timing_stage(
                                timing_enabled,
                                root,
                                canonical_path,
                                "extract.workspace.out_dirs",
                                extract_started.elapsed(),
                                "workspace_hit=false out_dirs=true",
                            );
                            return Ok(meta);
                        }
                        Err(RustMetadataError::CrateNotFound(_)) | Err(RustMetadataError::PathNotResolved(_)) => {}
                        Err(err) => return Err(err),
                    }
                    log_timing_stage(
                        timing_enabled,
                        root,
                        canonical_path,
                        "extract.workspace.out_dirs",
                        extract_started.elapsed(),
                        "workspace_hit=false out_dirs=true status=miss",
                    );
                }
                Err(err) => {
                    log_timing_stage(
                        timing_enabled,
                        root,
                        canonical_path,
                        "workspace.load.out_dirs",
                        load_started.elapsed(),
                        "out_dirs=true status=error",
                    );
                    if deferred_load_error.is_none() {
                        deferred_load_error = Some(err);
                    }
                }
            }
        }
    }

    let crate_name = crate_name_for_path(canonical_path);
    let dep_resolve_started = Instant::now();
    let dep_root = dependency_manifest_dir_for_crate(root, crate_name, registry_src_roots);
    log_timing_stage(
        timing_enabled,
        root,
        canonical_path,
        "dependency.resolve_manifest_dir",
        dep_resolve_started.elapsed(),
        crate_name,
    );
    if let Some(dep_root) = dep_root {
        let dep_root_display = dep_root.display().to_string();
        let dep_workspace = match inner.workspaces.entry((dep_root.clone(), true)) {
            Entry::Occupied(o) => o.into_mut(),
            Entry::Vacant(v) => {
                let load_started = Instant::now();
                let workspace = RustWorkspace::load_with_options(&dep_root, progress, true)?;
                log_timing_stage(
                    timing_enabled,
                    root,
                    canonical_path,
                    "workspace.load.dependency.out_dirs",
                    load_started.elapsed(),
                    dep_root_display.as_str(),
                );
                v.insert(workspace)
            }
        };
        let extract_started = Instant::now();
        let meta = extract_rust_item(dep_workspace, canonical_path);
        log_timing_stage(
            timing_enabled,
            root,
            canonical_path,
            "extract.workspace.dependency",
            extract_started.elapsed(),
            dep_root_display.as_str(),
        );
        return meta;
    }

    if let Some(err) = deferred_load_error {
        return Err(err);
    }

    Err(RustMetadataError::CrateNotFound(crate_name.to_string()))
}

impl RustMetadataCache {
    /// Create an empty cache.
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(CacheInner::default()),
        }
    }

    /// Return metadata for `canonical_path`, loading/extracting on cache miss.
    ///
    /// Lookup order is:
    /// 1. in-memory exact/alias hits
    /// 2. workspace extraction using canonical-path candidates
    /// 3. dependency-workspace extraction fallback
    /// 4. persisted disk-cache update for future sessions
    fn get_or_extract_inner(
        &self,
        manifest_dir: &Path,
        canonical_path: &str,
        registry_src_roots: Option<&[PathBuf]>,
        progress: &(dyn Fn(String) + Sync),
    ) -> Result<Arc<RustItemMetadata>, RustMetadataError> {
        let root = manifest_dir.canonicalize()?;
        let timing_enabled = rust_inspect_timing_enabled();
        let mut trace = CallTrace::new(timing_enabled, &root, canonical_path);
        let key_item = (root.clone(), canonical_path.to_owned());

        let mut inner = self.inner.lock().map_err(|e| RustMetadataError::LoadWorkspace {
            path: root.clone(),
            message: format!("metadata cache lock poisoned: {e}"),
        })?;

        let disk_load_started = Instant::now();
        ensure_disk_cache_loaded(&mut inner, &root)?;
        log_timing_stage(
            timing_enabled,
            &root,
            canonical_path,
            "disk_cache.ensure_loaded",
            disk_load_started.elapsed(),
            "",
        );

        if let Some(hit) = inner.items.get(&key_item) {
            trace.set_outcome("hit.memory.exact");
            return Ok(Arc::clone(hit));
        }

        let mut last_err = None;
        let mut meta = None;
        for candidate in canonical_path_candidates(canonical_path) {
            let candidate_key = (root.clone(), candidate.clone());
            if let Some(hit) = inner.items.get(&candidate_key) {
                let mut aliased = (*hit.as_ref()).clone();
                aliased.canonical_path = canonical_path.to_owned();
                let arc = Arc::new(aliased);
                inner.items.insert(key_item.clone(), Arc::clone(&arc));
                let persist_started = Instant::now();
                if let Err(err) = persist_item_to_disk_cache(&inner, &root, arc.as_ref())
                    && timing_enabled
                {
                    eprintln!(
                        "[rust-inspect-timing] root={} query={} stage=disk_cache.persist.alias_hit status=error err={err}",
                        root.display(),
                        canonical_path
                    );
                }
                log_timing_stage(
                    timing_enabled,
                    &root,
                    canonical_path,
                    "disk_cache.persist.alias_hit",
                    persist_started.elapsed(),
                    "",
                );
                trace.set_outcome("hit.memory.alias");
                return Ok(arc);
            }
            match extract_in_workspace_set(
                &mut inner,
                &root,
                candidate.as_str(),
                registry_src_roots,
                progress,
                timing_enabled,
            ) {
                Ok(found) => {
                    meta = Some(found);
                    break;
                }
                Err(err) => last_err = Some(err),
            }
        }
        let mut meta = meta.ok_or_else(|| {
            last_err
                .unwrap_or_else(|| RustMetadataError::CrateNotFound(crate_name_for_path(canonical_path).to_string()))
        })?;
        meta.canonical_path = canonical_path.to_owned();
        let arc = Arc::new(meta);
        inner.items.insert(key_item, Arc::clone(&arc));
        let persist_started = Instant::now();
        if let Err(err) = persist_item_to_disk_cache(&inner, &root, arc.as_ref())
            && timing_enabled
        {
            eprintln!(
                "[rust-inspect-timing] root={} query={} stage=disk_cache.persist.extracted status=error err={err}",
                root.display(),
                canonical_path
            );
        }
        log_timing_stage(
            timing_enabled,
            &root,
            canonical_path,
            "disk_cache.persist.extracted",
            persist_started.elapsed(),
            "",
        );
        trace.set_outcome("hit.extracted");
        Ok(arc)
    }

    pub fn get_or_extract(
        &self,
        manifest_dir: &Path,
        canonical_path: &str,
        progress: &(dyn Fn(String) + Sync),
    ) -> Result<Arc<RustItemMetadata>, RustMetadataError> {
        self.get_or_extract_inner(manifest_dir, canonical_path, None, progress)
    }

    /// Return metadata from memory/disk cache only.
    ///
    /// This does not trigger rust-analyzer workspace loading or extraction.
    pub fn get_cached(
        &self,
        manifest_dir: &Path,
        canonical_path: &str,
    ) -> Result<Option<CacheLookupHit>, RustMetadataError> {
        let root = manifest_dir.canonicalize()?;
        let key_item = (root.clone(), canonical_path.to_owned());
        let mut inner = self.inner.lock().map_err(|e| RustMetadataError::LoadWorkspace {
            path: root.clone(),
            message: format!("metadata cache lock poisoned: {e}"),
        })?;
        ensure_disk_cache_loaded(&mut inner, &root)?;

        if let Some(hit) = inner.items.get(&key_item) {
            return Ok(Some(CacheLookupHit {
                metadata: Arc::clone(hit),
                alias_used: false,
            }));
        }

        for candidate in canonical_path_candidates(canonical_path) {
            let candidate_key = (root.clone(), candidate.clone());
            if let Some(hit) = inner.items.get(&candidate_key) {
                let mut aliased = (*hit.as_ref()).clone();
                aliased.canonical_path = canonical_path.to_owned();
                let arc = Arc::new(aliased);
                inner.items.insert(key_item.clone(), Arc::clone(&arc));
                if let Err(err) = persist_item_to_disk_cache(&inner, &root, arc.as_ref()) {
                    tracing::warn!(
                        root = %root.display(),
                        query = %canonical_path,
                        error = %err,
                        "failed to persist rust-inspect disk cache after alias hit"
                    );
                    if rust_inspect_timing_enabled() {
                        eprintln!(
                            "[rust-inspect-timing] root={} query={} stage=disk_cache.persist.cached_alias status=error err={err}",
                            root.display(),
                            canonical_path
                        );
                    }
                }
                return Ok(Some(CacheLookupHit {
                    metadata: arc,
                    alias_used: true,
                }));
            }
        }
        Ok(None)
    }

    pub fn invalidate_manifest_dir(&self, manifest_dir: &Path) -> Result<(), RustMetadataError> {
        let root = manifest_dir.canonicalize()?;
        let mut inner = self.inner.lock().map_err(|e| RustMetadataError::LoadWorkspace {
            path: root.clone(),
            message: format!("metadata cache lock poisoned: {e}"),
        })?;
        inner
            .workspaces
            .retain(|(workspace_root, _), _| workspace_root != &root);
        inner.items.retain(|(workspace_root, _), _| workspace_root != &root);
        inner.disk_cache_state.remove(&root);
        Ok(())
    }

    #[doc(hidden)]
    pub fn get_or_extract_with_registry_src_roots(
        &self,
        manifest_dir: &Path,
        canonical_path: &str,
        registry_src_roots: &[PathBuf],
        progress: &(dyn Fn(String) + Sync),
    ) -> Result<Arc<RustItemMetadata>, RustMetadataError> {
        self.get_or_extract_inner(manifest_dir, canonical_path, Some(registry_src_roots), progress)
    }

    /// Seed metadata directly for tests without invoking rust-analyzer extraction.
    #[doc(hidden)]
    pub fn insert_test_item(&self, manifest_dir: &Path, metadata: RustItemMetadata) -> Result<(), RustMetadataError> {
        let root = manifest_dir.canonicalize()?;
        let key_item = (root, metadata.canonical_path.clone());
        let mut inner = self.inner.lock().map_err(|e| RustMetadataError::LoadWorkspace {
            path: manifest_dir.to_path_buf(),
            message: format!("metadata cache lock poisoned: {e}"),
        })?;
        inner.items.insert(key_item, Arc::new(metadata));
        Ok(())
    }
}

impl Default for RustMetadataCache {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    include!("cache_tests.rs");
}
