use sqlx::PgPool;

/// 创建 PostgreSQL 连接池。
///
/// `database_url` 由调用方（应用层）提供，core 库不再读取环境变量。
pub async fn get_db_client(database_url: &str) -> PgPool {
    PgPool::connect(database_url)
        .await
        .expect("database connection build failed")
}
