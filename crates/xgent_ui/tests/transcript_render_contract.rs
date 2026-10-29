//! M6.5 Transcript 渲染契约：顺序、Markdown 终稿、工具卡与通知。

use bevy::prelude::*;
use xgent_agent::Conversation;
use xgent_ui::chat_panel::MessageListMarker;
use xgent_ui::fonts::UiFonts;
use xgent_ui::kit::IconAssets;
use xgent_ui::markdown_render::MarkdownRendered;
use xgent_ui::theme::Theme;
use xgent_ui::tool_panel::{ToolCardMarker, ToolStatusLabelMarker};
use xgent_ui::transcript::Transcript;
use xgent_ui::transcript_render::{TranscriptBlockKey, TranscriptRenderPlugin, TranscriptSpacer};

#[derive(Resource)]
struct ListEntity(Entity);

fn app() -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Theme>()
        .init_resource::<xgent_settings::Localizer>()
        .insert_resource(UiFonts::fallback())
        .insert_resource(IconAssets::default())
        .insert_resource(Conversation::default())
        .insert_resource(Transcript::default())
        .add_plugins(TranscriptRenderPlugin);
    let list = app
        .world_mut()
        .spawn((MessageListMarker, xui::VirtualList::new(0, 120.0)))
        .id();
    app.insert_resource(ListEntity(list));
    app
}

fn keyed_blocks(app: &mut App) -> Vec<(u64, usize)> {
    let list = app.world().resource::<ListEntity>().0;
    let children = app
        .world()
        .get::<Children>(list)
        .map(|children| children.iter().collect::<Vec<_>>())
        .unwrap_or_default();
    children
        .into_iter()
        .filter_map(|child| {
            app.world()
                .get::<TranscriptBlockKey>(child)
                .map(|key| (key.turn_index, key.block_index))
        })
        .collect()
}

#[test]
fn user_and_streaming_assistant_render_in_order() {
    let mut app = app();
    {
        let mut transcript = app.world_mut().resource_mut::<Transcript>();
        let (turn, _) = transcript.push_user_message("hello".into());
        transcript.begin_assistant_message(turn);
        transcript.append_text_delta(turn, "streaming");
    }
    app.update();

    assert_eq!(keyed_blocks(&mut app), vec![(0, 0), (0, 1)]);
    let mut query = app.world_mut().query::<&Text>();
    let texts: Vec<String> = query.iter(app.world()).map(|text| text.0.clone()).collect();
    assert!(
        texts.iter().any(|text| text.contains("hello")),
        "用户消息应渲染：{texts:?}"
    );
    assert!(
        texts.iter().any(|text| text.contains("streaming")),
        "流式助手正文应渲染：{texts:?}"
    );
}

fn row_for_key(app: &App, key: (u64, usize)) -> Option<Entity> {
    let list = app.world().resource::<ListEntity>().0;
    app.world()
        .get::<Children>(list)
        .into_iter()
        .flatten()
        .find(|child| {
            app.world()
                .get::<TranscriptBlockKey>(**child)
                .is_some_and(|k| (k.turn_index, k.block_index) == key)
        })
        .copied()
}

#[test]
fn finalize_replaces_only_current_assistant_row() {
    let mut app = app();
    {
        let mut transcript = app.world_mut().resource_mut::<Transcript>();
        let (turn, _) = transcript.push_user_message("hi".into());
        transcript.begin_assistant_message(turn);
        transcript.append_text_delta(turn, "streaming");
    }
    app.update();
    let row_before = row_for_key(&app, (0, 1)).expect("assistant row");
    {
        let mut transcript = app.world_mut().resource_mut::<Transcript>();
        transcript.finalize_assistant(0);
    }
    app.update();
    let row_after = row_for_key(&app, (0, 1)).expect("assistant row after finalize");
    assert_eq!(row_before, row_after, "终稿替换不应重建消息行实体");
    let mut query = app.world_mut().query::<&MarkdownRendered>();
    assert!(query.iter(app.world()).count() >= 1);
}

#[test]
fn finalized_assistant_renders_markdown() {
    let mut app = app();
    {
        let mut transcript = app.world_mut().resource_mut::<Transcript>();
        let (turn, _) = transcript.push_user_message("hi".into());
        transcript.begin_assistant_message(turn);
        transcript.append_text_delta(turn, "## 标题\n\n正文 **粗体**");
        transcript.finalize_assistant(turn);
    }
    app.update();

    let mut query = app.world_mut().query::<&MarkdownRendered>();
    let count = query.iter(app.world()).count();
    assert!(count >= 1, "终稿助手消息应渲染 Markdown");
}

#[test]
fn tool_card_reflects_status_and_result() {
    let mut app = app();
    {
        let mut transcript = app.world_mut().resource_mut::<Transcript>();
        let (turn, _) = transcript.push_user_message("run".into());
        transcript.begin_assistant_message(turn);
        transcript.append_text_delta(turn, "before");
        transcript.push_tool_call(
            turn,
            "tc1".into(),
            "read_file".into(),
            "{\"path\":\"src/main.rs\"}".into(),
        );
        transcript.set_tool_progress("tc1", "partial");
        transcript.set_tool_result(turn, "tc1".into(), "done".into(), false);
        transcript.append_text_delta(turn, "after");
        transcript.finalize_assistant(turn);
    }
    app.update();

    let keys = keyed_blocks(&mut app);
    assert_eq!(keys, vec![(0, 0), (0, 1), (0, 2), (0, 4)]);

    let mut query = app.world_mut().query::<&ToolCardMarker>();
    let cards = query.iter(app.world()).count();
    assert_eq!(cards, 1, "应有一张工具卡");

    let mut query = app
        .world_mut()
        .query_filtered::<&Text, With<ToolStatusLabelMarker>>();
    let labels: Vec<String> = query.iter(app.world()).map(|text| text.0.clone()).collect();
    assert!(
        labels
            .iter()
            .any(|label| label.contains("完成") || label.contains("Completed")),
        "工具卡应显示完成态：{labels:?}"
    );
}

#[test]
fn notices_render_and_order_is_monotonic() {
    let mut app = app();
    {
        let mut transcript = app.world_mut().resource_mut::<Transcript>();
        let (turn, _) = transcript.push_user_message("hi".into());
        transcript.begin_assistant_message(turn);
        transcript.append_text_delta(turn, "ok");
        transcript.finalize_assistant(turn);
        transcript.push_compression_notice(turn, 1000, 500);
        transcript.push_notice(
            turn,
            "undo done".into(),
            xgent_ui::transcript::NoticeKind::Info,
        );
    }
    app.update();

    let keys = keyed_blocks(&mut app);
    assert_eq!(keys, vec![(0, 0), (0, 1), (0, 2), (0, 3)]);
    let mut query = app.world_mut().query::<&Text>();
    let texts: Vec<String> = query.iter(app.world()).map(|text| text.0.clone()).collect();
    assert!(texts.iter().any(|text| text.contains("undo done")));
}

#[test]
fn long_conversation_only_renders_visible_window() {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .init_resource::<Theme>()
        .init_resource::<xgent_settings::Localizer>()
        .insert_resource(UiFonts::fallback())
        .insert_resource(IconAssets::default())
        .insert_resource(Conversation::default())
        .insert_resource(Transcript::default())
        .add_plugins(TranscriptRenderPlugin);
    let mut computed = bevy::ui::ComputedNode::default();
    computed.size = Vec2::new(800.0, 600.0);
    computed.inverse_scale_factor = 1.0;
    let list = app
        .world_mut()
        .spawn((
            MessageListMarker,
            xui::VirtualList::new(0, 120.0),
            computed,
            bevy::ui::ScrollPosition(Vec2::new(0.0, 23_400.0)),
        ))
        .id();
    app.insert_resource(ListEntity(list));
    {
        let mut transcript = app.world_mut().resource_mut::<Transcript>();
        for index in 0..100 {
            let (turn, _) = transcript.push_user_message(format!("q{index}"));
            transcript.begin_assistant_message(turn);
            transcript.append_text_delta(turn, &format!("a{index}"));
            transcript.finalize_assistant(turn);
        }
    }
    app.update();

    let rendered = keyed_blocks(&mut app).len();
    assert!(rendered < 40, "200 个块只应渲染可见窗口，实际 {rendered}");
    let spacers = app
        .world_mut()
        .query::<&TranscriptSpacer>()
        .iter(app.world())
        .count();
    assert!(spacers >= 1, "滚出窗口应生成占位块");
}
