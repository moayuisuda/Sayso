//! `.sayso` 扩展包：扩展目录的 zip 归档，原样包含其中全部文件（代码、模型、运行环境）。
//!
//! 扩展包不做下载或安装步骤，拿到的是什么就装什么；不同系统、架构的版本由发布者分别打包。
//! 归档保留 unix 权限与符号链接，识别器自带的运行环境依赖这两项。扩展的用户数据不在扩展目录中，不会进入扩展包。

use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};

use zip::write::SimpleFileOptions;
use zip::ZipArchive;

use crate::error::CoreError;
use crate::plugin::{load_one, MANIFEST_FILE};
use crate::protocol::Manifest;

pub const FILE_EXTENSION: &str = "sayso";

/// 已解压、校验过清单、等待用户确认安装的扩展包。
pub(crate) struct PackageContents {
    pub manifest: Manifest,
    /// 解压后的扩展目录（临时位置）。
    directory: PathBuf,
    /// 扩展包内全部文件的总大小。
    pub byte_size: u64,
    staging: PathBuf,
}

fn invalid(error: impl std::fmt::Display) -> CoreError {
    CoreError::InvalidPackage(error.to_string())
}

/// 解压到 `staging` 并校验清单，供用户确认后安装。`staging` 与扩展目录在同一卷上，安装时直接移动。
pub(crate) fn open(package: &Path, staging: PathBuf) -> Result<PackageContents, CoreError> {
    let result = (|| {
        let mut archive = ZipArchive::new(File::open(package)?).map_err(invalid)?;
        // 只解出位于目标目录内的条目；保留权限与符号链接。
        archive.extract(&staging).map_err(invalid)?;
        let directory = extension_directory(&staging)?;
        let plugin = load_one(&directory)?;
        Ok(PackageContents {
            manifest: plugin.manifest,
            byte_size: size(&directory),
            directory,
            staging: staging.clone(),
        })
    })();
    if result.is_err() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    result
}

/// 安装到 `target`；已安装同一扩展时整体替换。
pub(crate) fn install(contents: PackageContents, target: &Path) -> Result<(), CoreError> {
    if target.exists() {
        std::fs::remove_dir_all(target)?;
    }
    if let Some(parent) = target.parent() {
        std::fs::create_dir_all(parent)?;
    }
    std::fs::rename(&contents.directory, target)?;
    discard(contents);
    Ok(())
}

/// 用户取消安装时清理临时文件。
pub(crate) fn discard(contents: PackageContents) {
    let _ = std::fs::remove_dir_all(&contents.staging);
}

/// 把扩展目录原样打包，归档根目录为扩展文件夹本身。
pub(crate) fn export(directory: &Path, destination: &Path) -> Result<(), CoreError> {
    let parent = directory.parent().ok_or_else(|| invalid("extension directory has no parent"))?;
    let mut writer = zip::ZipWriter::new(File::create(destination)?);
    let mut pending = vec![directory.to_path_buf()];
    while let Some(path) = pending.pop() {
        let name = path.strip_prefix(parent).map_err(invalid)?.to_string_lossy().replace('\\', "/");
        let metadata = std::fs::symlink_metadata(&path)?;
        let options = SimpleFileOptions::default().unix_permissions(permissions(&metadata));
        if metadata.is_symlink() {
            let target = std::fs::read_link(&path)?;
            writer.add_symlink(name, target.to_string_lossy(), options).map_err(invalid)?;
        } else if metadata.is_dir() {
            writer.add_directory(name, options).map_err(invalid)?;
            let mut children: Vec<_> =
                std::fs::read_dir(&path)?.map(|entry| entry.map(|entry| entry.path())).collect::<Result<_, _>>()?;
            children.sort();
            pending.extend(children.into_iter().rev());
        } else {
            writer.start_file(name, options).map_err(invalid)?;
            let mut file = File::open(&path)?;
            let mut buffer = vec![0; 1 << 20];
            loop {
                let read = file.read(&mut buffer)?;
                if read == 0 {
                    break;
                }
                writer.write_all(&buffer[..read])?;
            }
        }
    }
    writer.finish().map_err(invalid)?;
    Ok(())
}

#[cfg(unix)]
fn permissions(metadata: &std::fs::Metadata) -> u32 {
    use std::os::unix::fs::PermissionsExt;
    metadata.permissions().mode() & 0o777
}

#[cfg(not(unix))]
fn permissions(metadata: &std::fs::Metadata) -> u32 {
    if metadata.is_dir() {
        0o755
    } else {
        0o644
    }
}

/// 清单位于归档根目录，或位于唯一的顶层文件夹中（导出时的形式）。
fn extension_directory(staging: &Path) -> Result<PathBuf, CoreError> {
    if staging.join(MANIFEST_FILE).is_file() {
        return Ok(staging.to_path_buf());
    }
    let folders: Vec<PathBuf> = std::fs::read_dir(staging)?
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.is_dir() && path.file_name().is_some_and(|name| name != "__MACOSX"))
        .collect();
    match folders.as_slice() {
        [folder] if folder.join(MANIFEST_FILE).is_file() => Ok(folder.clone()),
        _ => Err(invalid(format!("{MANIFEST_FILE} not found"))),
    }
}

/// 目录内全部普通文件的大小之和，不跟随符号链接。
fn size(directory: &Path) -> u64 {
    let mut total = 0;
    let mut pending = vec![directory.to_path_buf()];
    while let Some(path) = pending.pop() {
        let Ok(metadata) = std::fs::symlink_metadata(&path) else { continue };
        if metadata.is_dir() {
            pending.extend(std::fs::read_dir(&path).into_iter().flatten().flatten().map(|entry| entry.path()));
        } else if metadata.is_file() {
            total += metadata.len();
        }
    }
    total
}

#[cfg(all(test, unix))]
mod tests {
    use std::os::unix::fs::PermissionsExt;

    use super::*;

    /// 导出再打开安装后，文件内容、可执行权限与符号链接都保留。
    #[test]
    fn export_open_install_round_trip_keeps_modes_and_symlinks() {
        let root = tempfile::tempdir().unwrap();
        let source = root.path().join("pack");
        std::fs::create_dir_all(source.join("model")).unwrap();
        std::fs::write(
            source.join(MANIFEST_FILE),
            r#"{"schemaVersion":3,"id":"com.example.pack","name":"Pack","capabilities":["audio"],"recognizer":{"title":"Local","command":["./run.sh"]}}"#,
        )
        .unwrap();
        std::fs::write(source.join("model/weights.bin"), vec![7u8; 4096]).unwrap();
        std::fs::write(source.join("run.sh"), "#!/bin/sh\n").unwrap();
        std::fs::set_permissions(source.join("run.sh"), std::fs::Permissions::from_mode(0o755)).unwrap();
        std::os::unix::fs::symlink("model/weights.bin", source.join("weights")).unwrap();

        let archive = root.path().join("pack.sayso");
        export(&source, &archive).unwrap();
        let contents = open(&archive, root.path().join("staging")).unwrap();
        assert_eq!(contents.manifest.id, "com.example.pack");
        assert!(contents.byte_size >= 4096);

        let target = root.path().join("Extensions/com.example.pack");
        install(contents, &target).unwrap();
        assert_eq!(std::fs::read(target.join("model/weights.bin")).unwrap().len(), 4096);
        assert_eq!(std::fs::metadata(target.join("run.sh")).unwrap().permissions().mode() & 0o777, 0o755);
        assert_eq!(std::fs::read_link(target.join("weights")).unwrap(), Path::new("model/weights.bin"));
        assert!(!root.path().join("staging").exists());
    }
}
