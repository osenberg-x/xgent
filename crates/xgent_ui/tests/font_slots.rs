//! v8 M4-T1 字体候选栈与等宽兜底回归（审计 A1 / 设计系统 C-FONT-1~3）。
//!
//! 缺陷 A1：`mono`（等宽）曾经是 macOS 独占特权路径——非 macOS 走
//! `Handle::default()`，而默认槽位已被 Inter 占据，**Windows/Linux 上的「等宽」
//! 其实是比例字体**。v8 改成「有序候选栈 + 内嵌兜底 + 解析日志」。
//!
//! 断言分两类：
//! - **纯函数**（候选顺序、内嵌兜底、失败标记）：与机器无关，必测。
//! - **本机解析**（真实系统字体集合）：证明 `mono` 命中的是真等宽而不是 `ui` 槽位。

use bevy::prelude::*;
use bevy::text::{FontCx, FontSource, GenericFontFamily};
use xgent_ui::fonts::{
    EMBEDDED_MONO_FAMILY, EMBEDDED_MONO_PATH, FontCandidate, FontOrigin, FontSlot, FontSpec,
    font_specs, resolve_slot,
};

/// 造一个候选栈规格（与平台表无关的合成数据）。
fn spec(candidates: &'static [FontCandidate]) -> FontSpec {
    FontSpec {
        slot: FontSlot::Mono,
        candidates,
        fallback_generic: GenericFontFamily::Monospace,
    }
}

/// A1：`mono` 在系统无等宽字体时回退到**内嵌 JetBrains Mono**，且不是 `ui` 槽位的家族。
#[test]
fn mono_falls_back_to_embedded_asset() {
    static CANDIDATES: &[FontCandidate] = &[
        FontCandidate::system("Definitely Not Installed Mono"),
        FontCandidate::embedded(EMBEDDED_MONO_FAMILY, EMBEDDED_MONO_PATH),
    ];
    let handle = Handle::default();
    let resolved = resolve_slot(
        &spec(CANDIDATES),
        &|_| false,
        &|candidate: &FontCandidate| {
            (candidate.embedded == Some(EMBEDDED_MONO_PATH)).then_some(handle.clone())
        },
    );
    assert_eq!(resolved.origin, FontOrigin::Embedded);
    assert_eq!(resolved.family, EMBEDDED_MONO_FAMILY);
    assert!(
        !resolved.family.contains("Inter"),
        "等宽槽位不得回退到 ui 槽位的比例字体（实测 {}）",
        resolved.family
    );
    assert!(
        matches!(resolved.source, FontSource::Handle(_)),
        "内嵌兜底应给出字体句柄（实测 {:?}）",
        resolved.source
    );
}

/// A2：候选栈**按顺序**命中——系统里存在第二个候选时取第二个。
#[test]
fn candidates_are_hit_in_order() {
    static CANDIDATES: &[FontCandidate] = &[
        FontCandidate::system("Missing Mono"),
        FontCandidate::system("Present Mono"),
        FontCandidate::system("Also Present Mono"),
    ];
    let resolved = resolve_slot(
        &spec(CANDIDATES),
        &|f| f == "Present Mono" || f == "Also Present Mono",
        &|_| None,
    );
    assert_eq!(resolved.origin, FontOrigin::System);
    assert_eq!(resolved.family, "Present Mono");
}

/// A3：候选全缺且无内嵌资产 → 标记**失败**（调用方记警告），不静默回退。
#[test]
fn failure_is_reported_not_silent() {
    let resolved = resolve_slot(&spec(&[]), &|_| false, &|_| None);
    assert_eq!(resolved.origin, FontOrigin::Failed);
    assert!(
        matches!(resolved.source, FontSource::Generic(_)),
        "失败兜底应是该槽位声明的通用族（实测 {:?}）",
        resolved.source
    );
}

/// A4：平台差异只在候选栈**数据**里——字体相关代码不得出现 `cfg!(target_os` 特权路径。
#[test]
fn no_platform_special_case_in_font_code() {
    let sources = [
        ("xgent_ui/src/fonts.rs", include_str!("../src/fonts.rs")),
        (
            "xgent_app/src/startup.rs",
            include_str!("../../xgent_app/src/startup.rs"),
        ),
    ];
    for (name, src) in sources {
        // 只看代码：去掉行注释（文档里可以引用这条约束本身）
        let code: String = src
            .lines()
            .map(|line| line.split("//").next().unwrap_or(line))
            .collect::<Vec<_>>()
            .join("\n");
        assert!(
            !code.contains("cfg!(target_os"),
            "{name} 出现了 cfg!(target_os 特权路径——平台差异只应在候选栈顺序数据里"
        );
        assert!(
            !code.contains("cfg!(windows") && !code.contains("cfg!(unix"),
            "{name} 出现了平台条件编译"
        );
    }
}

/// A5：内嵌 JetBrains Mono 资产就位，并登记为 `mono` 槽位的**内嵌兜底**候选。
#[test]
fn embedded_mono_asset_is_registered() {
    // 资产文件在仓库里
    let asset = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../xgent_app/assets/fonts/JetBrainsMono-Regular.ttf"
    );
    let meta = std::fs::metadata(asset).expect("内嵌 JetBrains Mono 资产应随仓库分发");
    assert!(
        meta.len() > 50_000,
        "内嵌等宽资产不应是空壳（{} 字节）",
        meta.len()
    );
    let head = std::fs::read(asset).expect("读取内嵌等宽资产");
    assert_eq!(
        &head[..4],
        &[0x00, 0x01, 0x00, 0x00],
        "应为 TrueType(sfnt) 字体"
    );

    // 且被登记为 mono 槽位候选栈里的内嵌兜底（最后一位）
    let mono = font_specs()
        .iter()
        .find(|s| s.slot == FontSlot::Mono)
        .expect("候选栈表应含 mono 槽位");
    let embedded: Vec<&FontCandidate> = mono
        .candidates
        .iter()
        .filter(|c| c.embedded.is_some())
        .collect();
    assert_eq!(
        embedded.len(),
        1,
        "mono 槽位应恰有一个内嵌兜底候选（实测 {}）",
        embedded.len()
    );
    assert_eq!(embedded[0].embedded, Some(EMBEDDED_MONO_PATH));
    assert_eq!(embedded[0].family, EMBEDDED_MONO_FAMILY);
    // 内嵌兜底是最后一位（系统优先）
    assert_eq!(
        mono.candidates.last().and_then(|c| c.embedded),
        Some(EMBEDDED_MONO_PATH),
        "内嵌兜底应是候选栈的最后一位"
    );
    // mono 的兜底通用族必须是等宽，绝不是比例字体
    assert_eq!(mono.fallback_generic, GenericFontFamily::Monospace);
}

/// A6：在**本机真实系统字体集合**上解析 `mono` 槽位，命中真等宽（不是 `ui` 的家族）。
#[test]
fn mono_resolves_to_real_monospace_on_this_machine() {
    let mut font_cx = FontCx::default();
    let families: Vec<String> = font_cx
        .context
        .collection
        .family_names()
        .map(str::to_string)
        .collect();
    let available = |family: &str| families.iter().any(|f| f == family);
    let handle = Handle::default();
    let embedded = |candidate: &FontCandidate| {
        (candidate.embedded == Some(EMBEDDED_MONO_PATH)).then_some(handle.clone())
    };

    let mono = font_specs()
        .iter()
        .find(|s| s.slot == FontSlot::Mono)
        .expect("候选栈表应含 mono 槽位");
    let resolved = resolve_slot(mono, &available, &embedded);

    assert_ne!(
        resolved.origin,
        FontOrigin::Failed,
        "mono 槽位不应解析失败（候选栈 {:?}）",
        mono.candidates
    );
    assert!(
        !resolved.family.contains("Inter"),
        "mono 槽位不得命中 ui 槽位的比例字体（实测 {}）",
        resolved.family
    );
    assert!(
        mono.candidates.iter().any(|c| c.family == resolved.family),
        "解析结果应来自候选栈（实测 {}）",
        resolved.family
    );
}
