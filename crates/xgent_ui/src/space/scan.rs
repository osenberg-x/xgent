//! 后台 L1 扫描任务：CPU 密集扫描不占用主线程，取消后丢弃结果。

use std::path::PathBuf;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use bevy::tasks::{AsyncComputeTaskPool, Task, TaskPool, block_on, poll_once};
use xgent_core::spatial::SpatialGraph;
use xgent_spatial::{LayoutConfig, ScanOptions, SpatialError, layout, scan_with_options};

/// 一次后台扫描。调用方每帧 `poll`，完成后取图；`cancel` 后结果被丢弃。
pub struct ScanJob {
    task: Task<Result<SpatialGraph, SpatialError>>,
    cancelled: Arc<AtomicBool>,
}

impl ScanJob {
    /// 在 Bevy 异步计算池上启动扫描；立即返回，不阻塞主线程。
    pub fn spawn(root: PathBuf, options: ScanOptions) -> Self {
        let pool = AsyncComputeTaskPool::get_or_init(TaskPool::default);
        let cancelled = Arc::new(AtomicBool::new(false));
        let flag = cancelled.clone();
        let task = pool.spawn(async move {
            if flag.load(Ordering::Relaxed) {
                return Err(SpatialError::Cancelled);
            }
            let graph = scan_with_options(&root, options)?;
            if flag.load(Ordering::Relaxed) {
                return Err(SpatialError::Cancelled);
            }
            let mut graph = graph;
            if !graph.clusters.is_empty() {
                layout(&mut graph, &LayoutConfig::default());
            }
            if flag.load(Ordering::Relaxed) {
                return Err(SpatialError::Cancelled);
            }
            Ok(graph)
        });
        Self { task, cancelled }
    }

    /// 请求取消；扫描线程可能继续跑完，但结果不会被调用方采用。
    pub fn cancel(&self) {
        self.cancelled.store(true, Ordering::Relaxed);
    }

    /// 非阻塞轮询；`None` 表示仍在运行。
    pub fn poll(&mut self) -> Option<Result<SpatialGraph, SpatialError>> {
        block_on(poll_once(&mut self.task))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::path::Path;
    use std::time::{Duration, Instant};

    fn write(path: &Path, contents: &str) {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("创建目录");
        }
        fs::write(path, contents).expect("写入文件");
    }

    fn options() -> ScanOptions {
        ScanOptions {
            l1: true,
            node_limit: 2000,
            ignore: true,
        }
    }

    fn wait(mut job: ScanJob) -> Option<Result<SpatialGraph, SpatialError>> {
        let deadline = Instant::now() + Duration::from_secs(20);
        loop {
            if let Some(result) = job.poll() {
                return Some(result);
            }
            assert!(Instant::now() < deadline, "扫描任务超时");
            std::thread::sleep(Duration::from_millis(5));
        }
    }

    #[test]
    fn scan_job_returns_graph() {
        let temp = tempfile::tempdir().expect("临时目录");
        let root = temp.path();
        write(
            &root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        );
        write(&root.join("src/main.rs"), "fn main() {}\n");

        let result = wait(ScanJob::spawn(root.to_path_buf(), options())).expect("应有结果");
        let graph = result.expect("扫描成功");
        assert!(!graph.nodes.is_empty());
    }

    #[test]
    fn cancel_discards_result() {
        let temp = tempfile::tempdir().expect("临时目录");
        let root = temp.path();
        write(
            &root.join("Cargo.toml"),
            "[package]\nname = \"demo\"\nversion = \"0.1.0\"\n",
        );
        for index in 0..2000 {
            write(&root.join(format!("src/file{index}.rs")), "x\n");
        }

        let job = ScanJob::spawn(root.to_path_buf(), options());
        job.cancel();
        let result = wait(job).expect("应有结果");
        assert!(matches!(result, Err(SpatialError::Cancelled)));
    }
}
