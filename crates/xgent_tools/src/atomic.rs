//! 文件写入与行范围编辑的共享实现。
//!
//! `write_file` 覆盖整文件、`edit_file` 替换行范围，二者都需要：
//! - 原子写（同目录临时文件 + rename，避免写崩时截断损坏原文件）
//! - 原文件备份（供回滚路径使用）
//! - 原子写的临时文件必须与目标同目录，跨目录 rename 在部分平台非原子。

use std::path::Path;

use tokio::io::AsyncWriteExt;

/// 工具层共用的内容大小上限（字节）：写入类工具拒绝超大内容。
///
/// 与 [`crate::MAX_TOOL_OUTPUT_BYTES`] 同值——单次工具调用进出的数据都不应
/// 超过此量级，既防撑爆 LLM 上下文，也防写爆磁盘。
pub const MAX_WRITE_BYTES: usize = 64 * 1024;

/// 备份文件后缀。备份与目标同目录，保证 rename 在同文件系统内原子。
pub const BACKUP_SUFFIX: &str = "xgent-bak";

/// 原子写文件：先写同目录临时文件再 rename。
///
/// 临时文件名由目标文件名派生（`<name>.xgent-tmp`），避免并发写同一路径时
/// 相互覆盖临时文件。
pub async fn atomic_write(full: &Path, content: &str) -> std::io::Result<()> {
    let tmp = tmp_path_for(full);
    {
        let mut f = tokio::fs::File::create(&tmp).await?;
        f.write_all(content.as_bytes()).await?;
        // 落盘后再 rename：flush 只保证写入内核缓冲，sync_all 才保证数据到盘，
        // 掉电场景下 rename 后仍可能读到空文件。
        f.flush().await?;
        f.sync_all().await?;
    }
    match tokio::fs::rename(&tmp, full).await {
        Ok(()) => Ok(()),
        Err(e) => {
            // 清理临时文件，避免残留垃圾
            let _ = tokio::fs::remove_file(&tmp).await;
            Err(e)
        }
    }
}

/// 备份文件路径。
pub fn backup_path_for(full: &Path) -> std::path::PathBuf {
    let mut name = full.file_name().unwrap_or_default().to_os_string();
    name.push(".");
    name.push(BACKUP_SUFFIX);
    full.with_file_name(name)
}

/// 原子写的临时文件路径。
fn tmp_path_for(full: &Path) -> std::path::PathBuf {
    let mut ext = full.extension().unwrap_or_default().to_os_string();
    ext.push("-xgent-tmp");
    full.with_extension(ext)
}

/// 备份原文件内容（若原文件存在）。
///
/// 返回备份路径；原文件不存在时返回 `None`（新建场景无可备份内容）。
pub async fn backup_existing(full: &Path) -> std::io::Result<Option<std::path::PathBuf>> {
    if !tokio::fs::try_exists(full).await? {
        return Ok(None);
    }
    let bak = backup_path_for(full);
    let content = tokio::fs::read(full).await?;
    let mut f = tokio::fs::File::create(&bak).await?;
    f.write_all(&content).await?;
    f.sync_all().await?;
    Ok(Some(bak))
}
