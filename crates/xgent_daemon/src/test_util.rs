//! 测试辅助：把全局配置的落盘位置隔离到临时目录。
//!
//! `ConfigCoordinator::write` 是会真的落盘的（这是它该有的行为），而默认路径就是
//! 用户自己的 `~/.xgent/agent/config.toml`——不隔离的话跑本 crate 的测试会把开发机
//! 的 provider 配置改成测试夹具（本轮验证者实测复现过一次），对用户来说等价于删数据。
//! 因此**任何会写全局配置的测试都必须先调 [`isolate_agent_dir`]**。
//!
//! 目录进程级只建一次且不删除（测试期间还要往里写，Drop 复原反而会指向已删除目录）；
//! 串行锁是因为 `set_var` 进程级生效，并发跑时两个测试互相指错目录会给出无意义的失败。

use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use xgent_settings_core::paths::ENV_AGENT_DIR;

static DIR: OnceLock<PathBuf> = OnceLock::new();
static LOCK: Mutex<()> = Mutex::new(());
/// 全局配置只有一个进程级路径，写它的测试必须**串行**：并行时 A 的写入会被 B 的
/// `empty_config()` 落盘覆盖，`global_write_lands_inside_the_isolated_dir` 实测就会
/// 读到别的测试的磁盘状态（断言 `left: None`）。
static CONFIG_IO: Mutex<()> = Mutex::new(());

/// 把 `XGENT_AGENT_DIR` 指到一个进程级临时目录（幂等）。返回该目录。
///
/// 只写会话/其他文件的用例用这个就够了；**写全局 `config.toml` 的用例还要另外持
/// 有 [`lock_config_io`] 到用例结束**，否则并发用例互相覆盖同一份配置文件。
pub(crate) fn isolate_agent_dir() -> &'static Path {
    // 守卫持有到函数结束：并发的第二个调用者不会读到尚未设置的 env
    let _guard = LOCK.lock().unwrap_or_else(|e| e.into_inner());
    DIR.get_or_init(|| {
        let tmp = tempfile::tempdir().expect("临时 agent 目录");
        let path = tmp.path().to_path_buf();
        // 持有到进程结束：TempDir 一析构就会删掉正在被测试写入的目录
        std::mem::forget(tmp);
        // SAFETY: 持有 LOCK，且整个测试进程只设一次
        unsafe { std::env::set_var(ENV_AGENT_DIR, &path) };
        path
    });
    DIR.get().expect("上面已初始化")
}

/// 全局配置写盘锁：`config.toml` 只有一个进程级路径，写它的用例必须串行到结束。
///
/// 守卫绑定到一个 `_io` 变量上即可（用例结束自动释放）。**不要在 async 用例里跨
/// `await` 持有它**（std Mutex 非 async 安全，`clippy::await_holding_lock` 会报）——
/// 目前只有同步的 `config_store` 用例需要它。
pub(crate) fn lock_config_io() -> std::sync::MutexGuard<'static, ()> {
    CONFIG_IO.lock().unwrap_or_else(|e| e.into_inner())
}
