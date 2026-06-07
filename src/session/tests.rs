use super::*;
use crate::context::ContextManager;

#[test]
fn test_sanitize_name() {
    assert_eq!(sanitize_name("my-session"), "my-session");
    assert_eq!(sanitize_name("hello world"), "hello_world");
    assert_eq!(sanitize_name("test/session"), "testsession");
    assert_eq!(sanitize_name("___"), "");
}

#[test]
fn test_serialize_deserialize_roundtrip() {
    let original = vec![
        SerializableMessage::System { content: "system prompt".to_string() },
        SerializableMessage::User { content: "hello".to_string() },
        SerializableMessage::Assistant {
            content: "hi there".to_string(),
            tool_calls: vec![
                SerializableToolCall {
                    id: "call_1".to_string(),
                    name: "shell".to_string(),
                    arguments: r#"{"command": "ls"}"#.to_string(),
                },
            ],
        },
        SerializableMessage::Tool {
            tool_call_id: "call_1".to_string(),
            content: r#"{"ok": true}"#.to_string(),
        },
    ];

    let json = serde_json::to_string_pretty(&original).unwrap();
    let deserialized: Vec<SerializableMessage> = serde_json::from_str(&json).unwrap();
    assert_eq!(original.len(), deserialized.len());

    for (orig, deser) in original.iter().zip(deserialized.iter()) {
        let chat_msg_orig: ChatMessage = orig.clone().into();
        let chat_msg_deser: ChatMessage = deser.clone().into();
        match (&chat_msg_orig, &chat_msg_deser) {
            (ChatMessage::User { content: a }, ChatMessage::User { content: b }) => {
                assert_eq!(a, b);
            }
            (ChatMessage::Assistant { content: a, tool_calls: tc_a },
             ChatMessage::Assistant { content: b, tool_calls: tc_b }) => {
                assert_eq!(a, b);
                assert_eq!(tc_a.len(), tc_b.len());
                if !tc_a.is_empty() {
                    assert_eq!(tc_a[0].id, tc_b[0].id);
                    assert_eq!(tc_a[0].name, tc_b[0].name);
                }
            }
            _ => {}
        }
    }
}

#[test]
fn test_session_manager_save_load_roundtrip() {
    use tempfile::TempDir;

    let dir = TempDir::new().unwrap();
    let root = dir.path().to_str().unwrap();

    let strategy = ContextStrategy::Auto {
        token_limit: 128_000,
        max_turns: 20,
        trigger_ratio: 0.7,
        enable_async_summary: true,
        enable_tool_pruning: true,
        tool_pruning_keep_recent: 3,
        tool_pruning_max_output_chars: 200,
    };
    let mut ctx = ContextManager::new("系统提示词", strategy);

    ctx.add_message(ChatMessage::user("你好"));
    ctx.add_message(ChatMessage::assistant("你好！有什么可以帮助你的吗？"));

    let sm = SessionManager::new(root, "/test/dir");

    let session = sm.save("test-session", &ctx).unwrap();
    assert_eq!(session.name, "test-session");
    assert_eq!(session.messages.len(), 2);
    assert_eq!(session.current_dir, "/test/dir");

    let list = sm.list().unwrap();
    assert_eq!(list.len(), 1);
    assert_eq!(list[0].name, "test-session");
    assert_eq!(list[0].message_count, 2);

    let loaded = sm.load("test-session").unwrap();
    assert_eq!(loaded.name, "test-session");
    assert_eq!(loaded.messages.len(), 2);

    let restored = sm.restore_messages(&loaded, "新的系统提示词");
    assert_eq!(restored.len(), 3);
    assert!(matches!(restored[0], ChatMessage::System { .. }));
    assert!(matches!(restored[1], ChatMessage::User { .. }));
    assert!(matches!(restored[2], ChatMessage::Assistant { .. }));

    let deleted = sm.delete("test-session").unwrap();
    assert!(deleted);
    assert_eq!(sm.list().unwrap().len(), 0);
}
