//! Coverage analysis for task versions that do not yet have a verified schema.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::path::Path;

use serde::Serialize;

use super::container::{Pack, TaskContainer, INDEX_MAGIC};
use super::schema::decode_prefix_diagnostic;
use super::{closest_schema_version, schema_for_version, supported_versions};

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceInfo {
    pub path: String,
    pub version: u32,
    pub export_version: u32,
    pub root_count: usize,
    pub pack_count: usize,
    pub size: u64,
    pub supported: bool,
    pub closest_version: u32,
    pub supported_versions: Vec<u32>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SourceVersion {
    pub version: u32,
    pub supported: bool,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct PackCoverage {
    pub pack: usize,
    pub roots: usize,
    pub exact_roots: usize,
    pub trailing_roots: usize,
    pub failed_roots: usize,
    pub bytes: u64,
    pub decoded_bytes: u64,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisIssue {
    pub pack: usize,
    pub root: usize,
    pub root_bytes: usize,
    pub offset: usize,
    pub kind: String,
    pub message: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct AnalysisReport {
    pub source: SourceInfo,
    pub baseline_version: u32,
    pub exact_roots: usize,
    pub trailing_roots: usize,
    pub failed_roots: usize,
    pub total_bytes: u64,
    pub decoded_bytes: u64,
    pub root_coverage: f64,
    pub byte_coverage: f64,
    pub exact_round_trip: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub first_issue: Option<AnalysisIssue>,
    pub packs: Vec<PackCoverage>,
}

#[derive(Debug)]
struct PackResult {
    coverage: PackCoverage,
    first_issue: Option<AnalysisIssue>,
}

pub fn inspect(path: impl AsRef<Path>) -> Result<SourceInfo, String> {
    let container = TaskContainer::open(path)?;
    Ok(source_info(&container))
}

/// Reads only the index magic and version. The UI uses this before the costly
/// pack verification so unsupported files are not hashed twice.
pub fn source_version(path: impl AsRef<Path>) -> Result<SourceVersion, String> {
    use std::io::Read;
    let path = path.as_ref();
    let mut file = std::fs::File::open(path).map_err(|error| format!("{}: {error}", path.display()))?;
    let mut header = [0u8; 8];
    file.read_exact(&mut header).map_err(|error| format!("{}: task index header is truncated: {error}", path.display()))?;
    let magic = u32::from_le_bytes(header[0..4].try_into().unwrap());
    if magic != INDEX_MAGIC {
        return Err(format!("{}: not a tasks.data index (magic 0x{magic:08x})", path.display()));
    }
    let version = u32::from_le_bytes(header[4..8].try_into().unwrap());
    Ok(SourceVersion { version, supported: supported_versions().contains(&version) })
}

pub fn analyze(path: impl AsRef<Path>, baseline_version: u32) -> Result<AnalysisReport, String> {
    let container = TaskContainer::open(path)?;
    let schema = schema_for_version(baseline_version)
        .map_err(|_| format!("v{baseline_version} is not a supported task-analysis baseline"))?;
    schema.validate()?;

    let next_pack = AtomicUsize::new(0);
    let workers = std::thread::available_parallelism().map(usize::from).unwrap_or(1).min(container.packs.len().max(1));
    let mut by_pack = std::thread::scope(|scope| -> Result<Vec<Option<PackResult>>, String> {
        let mut handles = Vec::with_capacity(workers);
        for _ in 0..workers {
            handles.push(scope.spawn(|| {
                let mut scanned = Vec::new();
                loop {
                    let pack_index = next_pack.fetch_add(1, Ordering::Relaxed);
                    let Some(pack) = container.packs.get(pack_index) else { break };
                    scanned.push((pack_index, analyze_pack(pack, pack_index, &schema, baseline_version)));
                }
                scanned
            }));
        }
        let mut results = (0..container.packs.len()).map(|_| None).collect::<Vec<_>>();
        for handle in handles {
            for (pack_index, result) in handle.join().map_err(|_| "Task analysis worker panicked")? {
                results[pack_index] = Some(result?);
            }
        }
        Ok(results)
    })?;

    let mut packs = Vec::with_capacity(container.packs.len());
    let mut first_issue = None;
    let mut exact_roots = 0;
    let mut trailing_roots = 0;
    let mut failed_roots = 0;
    let mut total_bytes = 0;
    let mut decoded_bytes = 0;
    for (pack_index, result) in by_pack.iter_mut().enumerate() {
        let result = result.take().ok_or_else(|| format!("Task pack {} was not analyzed", pack_index + 1))?;
        if first_issue.is_none() { first_issue = result.first_issue; }
        exact_roots += result.coverage.exact_roots;
        trailing_roots += result.coverage.trailing_roots;
        failed_roots += result.coverage.failed_roots;
        total_bytes += result.coverage.bytes;
        decoded_bytes += result.coverage.decoded_bytes;
        packs.push(result.coverage);
    }
    let root_count = exact_roots + trailing_roots + failed_roots;
    Ok(AnalysisReport {
        source: source_info(&container),
        baseline_version,
        exact_roots,
        trailing_roots,
        failed_roots,
        total_bytes,
        decoded_bytes,
        root_coverage: if root_count == 0 { 100.0 } else { exact_roots as f64 * 100.0 / root_count as f64 },
        byte_coverage: if total_bytes == 0 { 100.0 } else { decoded_bytes as f64 * 100.0 / total_bytes as f64 },
        exact_round_trip: root_count == exact_roots,
        first_issue,
        packs,
    })
}

fn source_info(container: &TaskContainer) -> SourceInfo {
    SourceInfo {
        path: container.index_path().display().to_string(),
        version: container.header.version,
        export_version: container.header.export_version,
        root_count: container.header.root_count as usize,
        pack_count: container.packs.len(),
        size: std::fs::metadata(container.index_path()).map(|metadata| metadata.len()).unwrap_or(0) + container.total_pack_bytes(),
        supported: supported_versions().contains(&container.header.version),
        closest_version: closest_schema_version(container.header.version),
        supported_versions: supported_versions().to_vec(),
    }
}

fn analyze_pack(pack: &Pack, pack_index: usize, schema: &super::schema::Schema, version: u32) -> Result<PackResult, String> {
    let data = std::fs::read(pack.path()).map_err(|error| format!("{}: {error}", pack.path().display()))?;
    let mut coverage = PackCoverage {
        pack: pack_index,
        roots: pack.root_count(),
        exact_roots: 0,
        trailing_roots: 0,
        failed_roots: 0,
        bytes: 0,
        decoded_bytes: 0,
    };
    let mut first_issue = None;
    for root in 0..pack.root_count() {
        let range = pack.root_range(root)?;
        let start = usize::try_from(range.start).map_err(|_| format!("{}: root {} offset is too large", pack.path().display(), root + 1))?;
        let end = usize::try_from(range.end).map_err(|_| format!("{}: root {} end is too large", pack.path().display(), root + 1))?;
        let bytes = data.get(start..end).ok_or_else(|| format!("{}: root {} range is outside the pack", pack.path().display(), root + 1))?;
        coverage.bytes += bytes.len() as u64;
        match decode_prefix_diagnostic(schema, bytes, version) {
            Ok((node, used)) => match node.encode() {
                Ok(encoded) if encoded == bytes[..used] && used == bytes.len() => {
                    coverage.exact_roots += 1;
                    coverage.decoded_bytes += used as u64;
                }
                Ok(encoded) if encoded == bytes[..used] => {
                    coverage.trailing_roots += 1;
                    coverage.decoded_bytes += used as u64;
                    first_issue.get_or_insert_with(|| issue(pack_index, root, bytes.len(), used, "trailing", format!("{} trailing bytes begin at offset 0x{used:X}", bytes.len() - used)));
                }
                Ok(_) => {
                    coverage.failed_roots += 1;
                    coverage.decoded_bytes += used as u64;
                    first_issue.get_or_insert_with(|| issue(pack_index, root, bytes.len(), used, "round_trip", "The baseline decoded this prefix, but encoding it did not reproduce the original bytes"));
                }
                Err(message) => {
                    coverage.failed_roots += 1;
                    coverage.decoded_bytes += used as u64;
                    first_issue.get_or_insert_with(|| issue(pack_index, root, bytes.len(), used, "round_trip", message));
                }
            },
            Err(failure) => {
                coverage.failed_roots += 1;
                coverage.decoded_bytes += failure.offset.min(bytes.len()) as u64;
                first_issue.get_or_insert_with(|| issue(pack_index, root, bytes.len(), failure.offset, "failed", failure.message));
            }
        }
    }
    Ok(PackResult { coverage, first_issue })
}

fn issue(pack: usize, root: usize, root_bytes: usize, offset: usize, kind: impl Into<String>, message: impl Into<String>) -> AnalysisIssue {
    AnalysisIssue { pack, root, root_bytes, offset, kind: kind.into(), message: message.into() }
}

#[cfg(test)]
mod tests {
    use std::time::{SystemTime, UNIX_EPOCH};

    use md5::{Digest, Md5};

    use super::*;

    #[test]
    fn unchanged_newer_header_scores_the_matching_baseline_exactly() {
        let source_path = Path::new(r"E:/Games/XtremeJade/element/data/tasks.data");
        if !source_path.is_file() { return }
        let source = TaskContainer::open(source_path).unwrap();
        let root = source.root(0, 0).unwrap();
        let folder = std::env::temp_dir().join(format!("jdide-task-analyze-{}-{}", std::process::id(), SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos()));
        std::fs::create_dir(&folder).unwrap();
        let index = folder.join("tasks.data");
        let pack = folder.join("tasks.data1");
        let mut pack_bytes = Vec::new();
        pack_bytes.extend_from_slice(&0x0693_4554u32.to_le_bytes());
        pack_bytes.extend_from_slice(&1u32.to_le_bytes());
        pack_bytes.extend_from_slice(&12u32.to_le_bytes());
        pack_bytes.extend_from_slice(&root);
        std::fs::write(&pack, &pack_bytes).unwrap();
        let digest: [u8; 16] = Md5::digest(&pack_bytes).into();
        let mut index_bytes = Vec::new();
        index_bytes.extend_from_slice(&0x6934_0304u32.to_le_bytes());
        index_bytes.extend_from_slice(&200u32.to_le_bytes());
        index_bytes.extend_from_slice(&source.header.export_version.to_le_bytes());
        index_bytes.extend_from_slice(&1u32.to_le_bytes());
        index_bytes.extend_from_slice(&1u32.to_le_bytes());
        index_bytes.extend_from_slice(&digest);
        std::fs::write(&index, index_bytes).unwrap();

        let peek = source_version(&index).unwrap();
        assert_eq!(peek.version, 200);
        assert!(!peek.supported);
        let info = inspect(&index).unwrap();
        assert!(!info.supported);
        assert_eq!(info.closest_version, 184);
        let report = analyze(&index, 165).unwrap();
        assert!(report.exact_round_trip);
        assert_eq!(report.exact_roots, 1);
        assert!(report.first_issue.is_none());
        std::fs::remove_dir_all(folder).unwrap();
    }

    #[test]
    fn extra_bytes_are_reported_at_the_first_trailing_offset() {
        let source_path = Path::new(r"E:/Games/XtremeJade/element/data/tasks.data");
        if !source_path.is_file() { return }
        let source = TaskContainer::open(source_path).unwrap();
        let mut bytes = source.root(0, 0).unwrap();
        let expected = bytes.len();
        bytes.extend_from_slice(&[0xAA, 0xBB, 0xCC, 0xDD]);
        let schema = schema_for_version(165).unwrap();
        let (_, used) = decode_prefix_diagnostic(&schema, &bytes, 165).unwrap();
        assert_eq!(used, expected);
    }
}
