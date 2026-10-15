//! 文件写入与行范围编辑的共享实现。
//!
//! `write_file` 覆盖整文件、`edit_file` 替换行范围，二者都需要：
//! - 原子写（同目录临时文件 + rename，避免写崩时截断损坏原文件）
//! - 改前状态由 [`crate::checkpoint`] 的检查点存储记录（旁置 `.xgent-bak`
//!   备份已废弃：无人读取、单槽位会被下次写入覆盖、还污染用户项目的工作区）
//! - 原子写的临时文件必须与目标同目录，跨目录 rename 在部分平台非原子。

use std::path::{Path, PathBuf};

use tokio::io::AsyncWriteExt;

/// 工具层共用的内容大小上限（字节）：写入类工具拒绝超大内容。
///
/// 与 [`crate::MAX_TOOL_OUTPUT_BYTES`] 同值——单次工具调用进出的数据都不应
/// 超过此量级，既防撑爆 LLM 上下文，也防写爆磁盘。
pub const MAX_WRITE_BYTES: usize = 64 * 1024;

/// 备份文件后缀。
///
/// 曾用于「写前把原文件复制一份到目标旁边」，该机制已被
/// [`crate::checkpoint`] 的检查点存储取代（旁置文件会污染用户项目的
/// `git status`，且单槽位会被下一次写入覆盖）。保留常量仅用于识别历史残留，
/// 本仓库不再创建此类文件。
pub const BACKUP_SUFFIX: &str = "xgent-bak";

/// 原子写字节内容：先写同目录临时文件再 rename。
///
/// 与 [`atomic_write`] 同一套语义（`sync_all` 后 rename、失败清理临时文件），
/// 供检查点 blob 与撤销恢复使用——撤销也是写，也必须原子。
pub async fn atomic_write_bytes(full: &Path, content: &[u8]) -> std::io::Result<()> {
    let tmp = tmp_path_for(full);
    {
        let mut f = tokio::fs::File::create(&tmp).await?;
        f.write_all(content).await?;
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

/// 原子写文件：先写同目录临时文件再 rename。
///
/// 临时文件名由目标文件名派生（`<name>.xgent-tmp`），避免并发写同一路径时
/// 相互覆盖临时文件。
pub async fn atomic_write(full: &Path, content: &str) -> std::io::Result<()> {
    atomic_write_bytes(full, content.as_bytes()).await
}

/// 原子写的临时文件路径。
fn tmp_path_for(full: &Path) -> PathBuf {
    let mut ext = full.extension().unwrap_or_default().to_os_string();
    ext.push("-xgent-tmp");
    full.with_extension(ext)
}
