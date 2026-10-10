//! Backups made before a save replaces a file: the replaced files go into one 7-Zip archive in a
//! `jdide_backups` folder next to the edited file, named `<file>_<YYYYMMDD-HHMMSS>.7z`.

use std::fs::File;
use std::path::{Path, PathBuf};

use sevenz_rust2::encoder_options::Lzma2Options;
use sevenz_rust2::{ArchiveEntry, ArchiveWriter, SourceReader};

/// The folder backups of `target` go into.
pub const FOLDER: &str = "jdide_backups";
/// LZMA2 preset: data files compress well already at a fast level.
const LEVEL: u32 = 5;
/// Independent chunks, so large files compress on all cores.
const CHUNK: u64 = 16 << 20;

/// Where the next backup of `target` goes (`…/jdide_backups/elements.data_20261010-203011.7z`).
pub fn archive_path(target: &Path) -> PathBuf {
    let name = target.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_else(|| "data".into());
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let folder = target.parent().unwrap_or(Path::new(".")).join(FOLDER);
    let first = folder.join(format!("{name}_{stamp}.7z"));
    if !first.exists() {
        return first;
    }
    (2..).map(|n| folder.join(format!("{name}_{stamp}-{n}.7z"))).find(|path| !path.exists()).unwrap()
}

/// Archives `files` (those that exist) as the backup of `target`, creating `jdide_backups` when needed.
/// Written under a temporary name and renamed when complete, so a failed backup leaves no archive.
pub fn archive(target: &Path, files: &[PathBuf]) -> Result<PathBuf, String> {
    let path = archive_path(target);
    let folder = path.parent().unwrap();
    std::fs::create_dir_all(folder).map_err(|error| format!("Could not create the backup folder {}: {error}", folder.display()))?;
    let partial = path.with_extension("7z.partial");
    let written = (|| -> Result<(), String> {
        let mut writer = ArchiveWriter::create(&partial).map_err(|error| error.to_string())?;
        let threads = std::thread::available_parallelism().map_or(1, |count| count.get().min(8)) as u32;
        writer.set_content_methods(vec![Lzma2Options::from_level_mt(LEVEL, threads, CHUNK).into()]);
        // One solid stream: a task set's many small packs then compress on all cores too.
        let mut entries = Vec::new();
        let mut sources = Vec::new();
        for file in files.iter().filter(|file| file.is_file()) {
            let name = file.file_name().map(|name| name.to_string_lossy().into_owned()).unwrap_or_default();
            sources.push(SourceReader::new(File::open(file).map_err(|error| format!("Could not read {}: {error}", file.display()))?));
            entries.push(ArchiveEntry::from_path(file, name));
        }
        if !entries.is_empty() {
            writer.push_archive_entries(entries, sources).map_err(|error| error.to_string())?;
        }
        writer.finish().map_err(|error| error.to_string())?;
        Ok(())
    })();
    if let Err(error) = written {
        let _ = std::fs::remove_file(&partial);
        return Err(format!("Could not write the backup {}: {error}", path.display()));
    }
    std::fs::rename(&partial, &path).map_err(|error| format!("Could not finish the backup {}: {error}", path.display()))?;
    Ok(path)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backups_are_7z_archives_in_their_own_folder() {
        let folder = std::env::temp_dir().join(format!("jdide-backup-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let target = folder.join("tasks.data");
        let pack = folder.join("tasks.data1");
        let data: Vec<u8> = (0..300_000u32).flat_map(|value| (value % 977).to_le_bytes()).collect();
        std::fs::write(&target, &data).unwrap();
        std::fs::write(&pack, b"pack one").unwrap();
        let first = archive(&target, &[target.clone(), pack.clone(), folder.join("missing")]).unwrap();
        assert_eq!(first.parent().unwrap().file_name().unwrap(), FOLDER);
        let name = first.file_name().unwrap().to_string_lossy().into_owned();
        assert!(name.starts_with("tasks.data_") && name.ends_with(".7z"), "{name}");
        assert!(std::fs::metadata(&first).unwrap().len() < data.len() as u64 / 4, "compressed");
        // A second backup in the same second gets its own name.
        let again = archive(&target, &[target.clone()]).unwrap();
        assert_ne!(again, first);

        let out = folder.join("out");
        sevenz_rust2::decompress_file(&first, &out).unwrap();
        assert_eq!(std::fs::read(out.join("tasks.data")).unwrap(), data);
        assert_eq!(std::fs::read(out.join("tasks.data1")).unwrap(), b"pack one");
        assert!(!out.join("missing").exists());
        let _ = std::fs::remove_dir_all(&folder);
    }
}

