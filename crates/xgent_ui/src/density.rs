//! 密度档位（v8 M2b）：把「界面疏密」变成一次属性写回，而不是重建节点。
//!
//! 做法：每个 `Node` 首次被本模块看到时，把它的**基线**间距（padding / margin /
//! row_gap / column_gap）记进 [`DensityBase`]，随后按当前档位写回缩放值。密度变化时
//! 再按同一份基线重算一遍。因此：
//!
//! - **不累乘**：写入始终以基线为源，重复应用结果相同；
//! - **不重建**：只改 `Node` 字段，符合「状态是权威、呈现是派生」；
//! - **新节点即合档**：紧凑档下新建的节点首帧就按紧凑值出现（基线仍是令牌原值，
//!   因为业务代码写的永远是 `space::*` 常量）。
//!
//! 只缩放间距。字号、图标尺寸、`Node.width/height`、边框宽度都不参与——
//! 依据 `docs/comet/changes/v8-m2b-density-hover/brief.md` 决策 D-2。

use bevy::prelude::*;

use crate::theme::{Density, ThemeHandle};

/// 节点的间距基线（密度生效前的原始值）。
///
/// 只记录会被密度缩放的四个字段；`Val::Percent`/`Val::Auto` 也一并记下，
/// 写回时原样保留。
#[derive(Component, Debug, Clone, Copy, PartialEq)]
pub struct DensityBase {
    /// 基线内边距。
    padding: UiRect,
    /// 基线外边距。
    margin: UiRect,
    /// 基线行间距。
    row_gap: Val,
    /// 基线列间距。
    column_gap: Val,
}

impl DensityBase {
    /// 取节点当前值作为基线。
    pub fn capture(node: &Node) -> Self {
        Self {
            padding: node.padding,
            margin: node.margin,
            row_gap: node.row_gap,
            column_gap: node.column_gap,
        }
    }

    /// 基线在 `density` 档位下的取值。
    pub fn scaled(&self, density: Density) -> (UiRect, UiRect, Val, Val) {
        (
            scale_rect(self.padding, density),
            scale_rect(self.margin, density),
            scale_val(self.row_gap, density),
            scale_val(self.column_gap, density),
        )
    }

    /// 把基线在当前档位下的值写回节点；返回是否真的改了。
    ///
    /// 「值不同才写」避免无谓的组件变更（`Node` 变脏会触发重新布局）。
    pub fn write(&self, node: &mut Node, density: Density) -> bool {
        let (padding, margin, row_gap, column_gap) = self.scaled(density);
        let mut changed = false;
        if node.padding != padding {
            node.padding = padding;
            changed = true;
        }
        if node.margin != margin {
            node.margin = margin;
            changed = true;
        }
        if node.row_gap != row_gap {
            node.row_gap = row_gap;
            changed = true;
        }
        if node.column_gap != column_gap {
            node.column_gap = column_gap;
            changed = true;
        }
        changed
    }
}

/// 单个 `Val` 的换算：只动 `Val::Px`，其余原样返回。
fn scale_val(value: Val, density: Density) -> Val {
    match value {
        Val::Px(px) => Val::Px(density.scale(px)),
        other => other,
    }
}

fn scale_rect(rect: UiRect, density: Density) -> UiRect {
    UiRect {
        left: scale_val(rect.left, density),
        right: scale_val(rect.right, density),
        top: scale_val(rect.top, density),
        bottom: scale_val(rect.bottom, density),
    }
}

/// 为新出现的节点记基线，并立即按当前档位写回。
///
/// 已记过基线的节点不会再进入这个查询，所以稳态下这个系统是空转。
fn capture_density_bases(
    handle: Res<ThemeHandle>,
    mut nodes: Query<(Entity, &mut Node), Without<DensityBase>>,
    mut commands: Commands,
) {
    let density = handle.density;
    for (entity, mut node) in nodes.iter_mut() {
        let base = DensityBase::capture(&node);
        base.write(&mut node, density);
        commands.entity(entity).insert(base);
    }
}

/// 密度变化时按基线重算全部节点。`ThemeHandle` 是唯一信号源（同 `generation`）。
fn apply_density_on_change(handle: Res<ThemeHandle>, mut nodes: Query<(&DensityBase, &mut Node)>) {
    if !handle.is_changed() {
        return;
    }
    let density = handle.density;
    for (base, mut node) in nodes.iter_mut() {
        base.write(&mut node, density);
    }
}

/// 密度插件：注册基线记录与重排系统。
pub struct DensityPlugin;

impl Plugin for DensityPlugin {
    fn build(&self, app: &mut App) {
        app.init_resource::<ThemeHandle>().add_systems(
            Update,
            (capture_density_bases, apply_density_on_change)
                .chain()
                // 先让主题同步把新档位写进 `ThemeHandle`，本帧即可重排
                .after(crate::theme::sync_theme_from_config),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let mut app = App::new();
        app.add_plugins(MinimalPlugins).add_plugins(DensityPlugin);
        app
    }

    fn set_density(app: &mut App, density: Density) {
        let mut handle = app.world_mut().resource_mut::<ThemeHandle>();
        if handle.density != density {
            handle.density = density;
            handle.generation = handle.generation.wrapping_add(1);
        }
    }

    fn spawn_node(app: &mut App, padding: f32, row_gap: f32) -> Entity {
        app.world_mut()
            .spawn(Node {
                padding: UiRect::all(Val::Px(padding)),
                row_gap: Val::Px(row_gap),
                ..default()
            })
            .id()
    }

    fn node(app: &App, entity: Entity) -> Node {
        app.world().get::<Node>(entity).unwrap().clone()
    }

    /// A2：切到紧凑后间距按 0.75 收紧。
    #[test]
    fn compact_tightens_padding_and_gap() {
        let mut app = app();
        let entity = spawn_node(&mut app, 24.0, 20.0);
        app.update();
        assert_eq!(node(&app, entity).padding.left, Val::Px(24.0));

        set_density(&mut app, Density::Compact);
        app.update();
        let n = node(&app, entity);
        assert_eq!(n.padding.left, Val::Px(18.0), "24 应缩到 18");
        assert_eq!(n.padding.top, Val::Px(18.0));
        assert_eq!(n.row_gap, Val::Px(15.0), "20 应缩到 15");
    }

    /// A3：重复应用不累乘；切回舒适回到基线。
    #[test]
    fn density_is_idempotent_and_reversible() {
        let mut app = app();
        let entity = spawn_node(&mut app, 16.0, 8.0);
        app.update();

        set_density(&mut app, Density::Compact);
        for _ in 0..5 {
            app.update();
            assert_eq!(
                node(&app, entity).padding.left,
                Val::Px(12.0),
                "不应继续缩小"
            );
        }
        // 再次确认同一档位（generation 不动）也不改变数值
        app.update();
        assert_eq!(node(&app, entity).row_gap, Val::Px(6.0));

        set_density(&mut app, Density::Comfortable);
        app.update();
        assert_eq!(node(&app, entity).padding.left, Val::Px(16.0), "应回到基线");
        assert_eq!(node(&app, entity).row_gap, Val::Px(8.0));
    }

    /// 紧凑档下新建的节点首帧即合档（基线仍是令牌原值）。
    #[test]
    fn nodes_spawned_while_compact_are_scaled_on_first_frame() {
        let mut app = app();
        set_density(&mut app, Density::Compact);
        app.update();

        let entity = spawn_node(&mut app, 12.0, 4.0);
        app.update();
        let n = node(&app, entity);
        assert_eq!(n.padding.left, Val::Px(9.0));
        assert_eq!(n.row_gap, Val::Px(3.0));

        // 基线仍在，切回舒适能还原成令牌原值
        set_density(&mut app, Density::Comfortable);
        app.update();
        assert_eq!(node(&app, entity).padding.left, Val::Px(12.0));
    }

    /// 非 Px 值（Percent/Auto）与 0 值原样保留；width/height 不参与缩放。
    #[test]
    fn non_px_values_and_fixed_sizes_are_untouched() {
        let mut app = app();
        let entity = app
            .world_mut()
            .spawn(Node {
                width: Val::Px(52.0),
                height: Val::Px(40.0),
                padding: UiRect {
                    left: Val::Percent(10.0),
                    right: Val::Auto,
                    top: Val::Px(8.0),
                    bottom: Val::Px(0.0),
                },
                margin: UiRect::horizontal(Val::Auto),
                ..default()
            })
            .id();
        app.update();
        set_density(&mut app, Density::Compact);
        app.update();

        let n = node(&app, entity);
        assert_eq!(n.width, Val::Px(52.0), "固定宽度不缩放");
        assert_eq!(n.height, Val::Px(40.0), "固定高度不缩放");
        assert_eq!(n.padding.left, Val::Percent(10.0), "Percent 原样");
        assert_eq!(n.padding.right, Val::Auto, "Auto 原样");
        assert_eq!(n.padding.top, Val::Px(6.0), "8 缩到 6");
        assert_eq!(n.padding.bottom, Val::Px(0.0), "0 仍是 0");
        assert_eq!(n.margin.left, Val::Auto, "margin Auto 原样");
    }

    /// 稳态下没有待记录基线的节点（查询为空），避免每帧空转出一份开销。
    #[test]
    fn every_node_carries_density_base_after_first_frame() {
        let mut app = app();
        spawn_node(&mut app, 8.0, 4.0);
        app.update();

        let mut q = app
            .world_mut()
            .query_filtered::<Entity, (With<Node>, Without<DensityBase>)>();
        assert_eq!(
            q.iter(app.world()).count(),
            0,
            "首帧后不应还有缺基线的 Node"
        );
    }
}
