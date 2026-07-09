use sqlx::PgPool;

/// 会话管理服务：负责会话元数据的 CRUD。
///
/// 当前为骨架实现，后续可扩展：
/// - 创建/删除会话
/// - 更新会话标题
/// - 列出用户会话
/// - 查询会话元数据
pub struct SessionService {
    db: PgPool,
}

impl SessionService {
    pub fn new(db: PgPool) -> Self {
        Self { db }
    }
}
