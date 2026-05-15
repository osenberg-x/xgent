# XGent MVP-1 Implementation Plan

## Workspace Structure

```
xgent/Cargo.toml (workspace)
crates/
  xgent_app/       (bin)  main entry
  xgent_ui/        (lib)  independent UI library
  xgent_agent/     (lib)  agent engine
  xgent_provider/  (lib)  provider + protocol
  xgent_mcp/       (lib)  MCP client
  xgent_tools/     (lib)  tools + security
  xgent_settings/  (lib)  config persistence
```

## Dependency Graph (acyclic)

xgent_ui -> xgent_agent -> xgent_provider
xgent_ui -> xgent_tools
xgent_agent -> xgent_tools
xgent_mcp -> xgent_settings
xgent_app -> all

## Crate Designs

### xgent_settings
- Based on bevy_settings PreferencesPlugin + SettingsGroup derive
- Key types: ProviderSettings, ProviderEntrySettings, McpServerSettings
- Auto TOML read/write

### xgent_provider
- Modules: provider.rs (trait), registry.rs, openai_compat.rs, chat_types.rs, router.rs, cost.rs, events.rs
- LLMProvider trait: id(), list_models(), chat()->ChatStream, health_check()
- ChatStream: tokio mpsc, events: Delta/ToolCall/Done
- OpenAI compat: POST chat/completions, SSE parse, supports OpenAI/DeepSeek/Ollama etc

### xgent_tools
- Modules: definition.rs (Tool enum, ToolCallRequest/Result as Message), executor.rs, security.rs, builtins/
- Tool: ReadFile, WriteFile, SearchFiles, RunCommand, Git*, McpTool
- Security: Read/Search/Git->Approved, Write/Run->NeedsConfirmation, blocked->Denied

### xgent_mcp
- Modules: client.rs, transport.rs (stdio), connection_manager.rs, tool_bridge.rs, types.rs
- McpClient: connect(), list_tools(), call_tool(), shutdown()
- MVP-1: stdio only, tools bridge only

### xgent_agent
- Modules: agent.rs (Component), orchestrator.rs, conversation.rs (loop), context.rs, chat_format.rs, events.rs
- Loop: build context -> Provider.chat() -> parse response -> if ToolCall then execute -> loop
- ECS bridge: tokio mpsc channel wrapped as Resource, Bevy System polls each frame

### xgent_ui (independent library)
- Modules: lib.rs (XgentUiPlugins PluginGroup), theme.rs, layout.rs, chat_panel.rs, tool_panel.rs, file_panel.rs, status_bar.rs, command_palette.rs, settings_panel.rs, confirm_dialog.rs, components/, resources.rs
- PluginGroup: FeathersPlugins + XgentUiCorePlugin
- Layout: top bar(40px) + main(flex:1, file preview + agent sidebar 360px) + status bar(24px)
- Chat: message list (virtual scroll) + EditableText input
- Confirm dialog: overlay for WriteFile/RunCommand

### xgent_app
- Assembles DefaultPlugins + all XgentPlugins + Startup init

## Steps

1. workspace skeleton (7 crate stubs) -> cargo check
2. xgent_settings -> TOML generation
3. xgent_provider -> LLMProvider + OpenAI adapter + streaming
4. xgent_tools -> Tool + SecurityPolicy + executor
5. xgent_agent -> conversation loop + ECS bridge
6. xgent_ui -> 4-zone layout + chat panel + confirm dialog
7. xgent_mcp -> stdio + McpClient + tool bridge
8. xgent_app -> assembly + e2e test

## Files to modify
- E:/ws/xgent/Cargo.toml (rewrite as workspace)
- Delete E:/ws/xgent/src/main.rs

## Files to create (Step 1)
- E:/ws/xgent/crates/xgent_app/Cargo.toml + src/main.rs
- E:/ws/xgent/crates/xgent_ui/Cargo.toml + src/lib.rs
- E:/ws/xgent/crates/xgent_agent/Cargo.toml + src/lib.rs
- E:/ws/xgent/crates/xgent_provider/Cargo.toml + src/lib.rs
- E:/ws/xgent/crates/xgent_mcp/Cargo.toml + src/lib.rs
- E:/ws/xgent/crates/xgent_tools/Cargo.toml + src/lib.rs
- E:/ws/xgent/crates/xgent_settings/Cargo.toml + src/lib.rs
