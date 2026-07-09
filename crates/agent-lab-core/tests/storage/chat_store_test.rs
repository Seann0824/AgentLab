use agent_lab_core::db::get_db_client;
use agent_lab_core::services::chat_dto::ChatMessage;
use agent_lab_core::storage::ChatStore;
use chrono::Local;

/// 该测试需要本地 PostgreSQL 服务，默认随 `cargo test` 运行。
/// 运行前请确保 `.env` 中 DATABASE_URL 配置正确。
#[tokio::test]
async fn test_chat_store_session_and_message_crud() {
    dotenvy::dotenv().ok();
    let database_url = std::env::var("DATABASE_URL").expect("DATABASE_URL not set");
    let db = get_db_client(&database_url).await;
    let store = ChatStore::new(db.clone());

    let session_id = uuid::Uuid::new_v4().to_string();
    let now = Local::now().timestamp();

    // 清理可能遗留的数据
    let _ = store.delete_session(&session_id).await;

    // 创建会话
    store
        .create_session(&session_id, "default_user", None, now, now)
        .await
        .expect("create session failed");

    // 添加用户消息
    let user_msg = ChatMessage::new_user(
        uuid::Uuid::new_v4().to_string(),
        "hello persistence".to_string(),
    );
    store
        .add_message(&session_id, &user_msg, 1)
        .await
        .expect("add user message failed");

    // 添加 assistant 消息
    let assistant_msg = ChatMessage {
        id: uuid::Uuid::new_v4().to_string(),
        role: "assistant".to_string(),
        content: "hi there".to_string(),
        timestamp: Local::now().timestamp(),
        tool_call_id: None,
        tool_calls: None,
        metadata: None,
    };
    store
        .add_message(&session_id, &assistant_msg, 2)
        .await
        .expect("add assistant message failed");

    // 读取历史
    let messages = store
        .get_messages(&session_id)
        .await
        .expect("get messages failed");
    assert_eq!(messages.len(), 2);
    assert_eq!(messages[0].content, "hello persistence");
    assert_eq!(messages[1].content, "hi there");

    // 列会话
    let sessions = store
        .list_sessions("default_user", 100)
        .await
        .expect("list sessions failed");
    assert!(sessions.iter().any(|s| s.id == session_id));

    // 清理
    let deleted = store.delete_session(&session_id).await.expect("delete failed");
    assert!(deleted);
}
