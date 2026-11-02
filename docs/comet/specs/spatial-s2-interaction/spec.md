# spatial-s2-interaction Specification

## Purpose

在已交付的 3D 首页 S1（`3d-home-view`）之上补齐 S2 交互联动：键盘遍历、搜索飞行、2D↔3D 选中同步、Esc 逐级回退与选中反馈。本 capability 只描述 S2 新增行为；S1 的视口/相机/打开/降级行为保持既有验收。

### Requirement: 键盘遍历

系统 SHALL 允许在宇宙可见、搜索未打开且文本输入框无焦点时，用 `Tab`/`Shift+Tab` 在当前图的可见节点间循环选中；顺序按屏幕极角可预期并回绕；选中时相机 target 跟随选中节点，yaw/pitch/dist 不变。

#### Scenario: Tab 遍历并回绕

- **WHEN** 用户连续按 `Tab` 或 `Shift+Tab`
- **THEN** 选中态依次移动到下一个/上一个可见节点，到末尾后回绕
- 验收：A1

#### Scenario: 遍历不旋转相机

- **WHEN** 用户用 `Tab` 切换选中节点
- **THEN** 相机 target 跟随选中节点，yaw、pitch 与 dist 保持不变
- 验收：A1

#### Scenario: 搜索或输入焦点阻止遍历

- **WHEN** 搜索框打开或任意文本输入框拥有焦点
- **THEN** `Tab`/`Shift+Tab` 不改变 3D 选中态
- 验收：A1

### Requirement: 搜索飞行

系统 SHALL 在宇宙可见且文本输入框无焦点时用 `/` 打开视口内搜索；搜索只覆盖当前图的 File 节点，按 label/path 子串过滤；`↑`/`↓` 移动结果，`Enter` 选中结果并把相机 target/dist 调整到该星体，不打开编辑器；`Esc` 只关闭搜索。

#### Scenario: 搜索并飞行定位

- **WHEN** 用户按 `/`，输入文件名片段并选择一条结果按 Enter
- **THEN** 搜索关闭，该文件星体被选中，相机定位到它，编辑器保持关闭
- 验收：A2

#### Scenario: 搜索后再次 Enter 打开

- **WHEN** 搜索飞行完成后用户再次按 Enter
- **THEN** 2D 编辑器打开搜索选中的同一路径
- 验收：A3

#### Scenario: 搜索 Esc 只关搜索

- **WHEN** 搜索打开时按 Esc
- **THEN** 搜索关闭，3D 选中态与对话均保持不变
- 验收：A5

#### Scenario: 无匹配结果

- **WHEN** 查询没有任何 File 节点匹配
- **THEN** 结果列表为空，Enter 不选中任何节点，Esc 仍可关闭搜索
- 验收：A2

### Requirement: 2D 与 3D 联动

系统 SHALL 在编辑器活动文件变化时，在图内选中同路径的 File 星体并保持相机不动；编辑器活动文件不在图中时清除 3D 选中；图中选中 File 后按 Enter/双击仍打开同一文件到 2D 编辑器。

#### Scenario: 编辑器切换文件同步选中

- **WHEN** 用户在编辑器中切换活动文件，且该文件在空间图中
- **THEN** 对应 File 星体进入选中态，相机 target/yaw/pitch/dist 不变
- 验收：A4

#### Scenario: 图中不存在当前文件

- **WHEN** 编辑器活动文件不在空间图中
- **THEN** 3D 选中态被清除，不保留指向其他文件的选中
- 验收：A4

#### Scenario: 3D 选中打开同一文件

- **WHEN** 用户选中一个 File 星体并按 Enter 或双击
- **THEN** 2D 编辑器打开的路径与该星体路径一致
- 验收：A3

### Requirement: Esc 逐级回退

系统 SHALL 按以下顺序处理宇宙相关 Esc，不得穿透到对话中断：搜索打开时只关搜索；编辑器/差异/终端可见时回到宇宙并保留 3D 选中；宇宙可见且有选中时只清除选中；以上都没有且宇宙可见时才中断对话。

#### Scenario: 搜索优先关闭

- **WHEN** 搜索打开时按 Esc
- **THEN** 只关闭搜索，不关闭编辑器、不清选中、不中断对话
- 验收：A5

#### Scenario: 工作区先回宇宙

- **WHEN** 编辑器、差异或终端可见时按 Esc
- **THEN** 回到 3D 宇宙，选中态保留
- 验收：A5

#### Scenario: 选中先清除

- **WHEN** 宇宙可见且有选中节点时按 Esc
- **THEN** 只清除选中，不中断对话
- 验收：A5

#### Scenario: 空宇宙才中断对话

- **WHEN** 宇宙可见、搜索关闭且没有选中节点时按 Esc
- **THEN** 才向当前对话发送中断
- 验收：A5

### Requirement: 选中反馈

系统 SHALL 在悬停星体 120ms 后显示 label/path tooltip；选中星体后显示详情卡（label、path、节点类型与 Enter 提示），详情卡不拦截视口拾取，切换或清除选中时同步更新/消失；L1 图不要求 LOC。

#### Scenario: 悬停 tooltip

- **WHEN** 指针在星体上停留至少 120ms
- **THEN** 显示该节点的 label/path tooltip，移开后消失
- 验收：A7

#### Scenario: 选中详情卡

- **WHEN** 用户选中一个星体
- **THEN** 详情卡显示 label/path/类型与 Enter 提示；切换或清除选中时同步更新/消失
- 验收：A7

### Requirement: S2 只在宇宙模式生效

系统 SHALL 将键盘遍历、搜索飞行与 2D↔3D 联动限制在可见的 `LayoutMode::Universe` 空间视图；经典 2D 回退下这些输入不改变布局、不打开 3D 搜索，MVP-1 的对话、编辑器、终端、抽屉、设置与会话历史行为保持不变。

#### Scenario: 经典回退不触发 S2

- **WHEN** 用户处于经典 2D 回退（`three_d_home=false`）并按 `/` 或 `Tab`
- **THEN** 不打开 3D 搜索、不切换 3D 选中，布局与 MVP-1 功能不受影响
- 验收：A6
