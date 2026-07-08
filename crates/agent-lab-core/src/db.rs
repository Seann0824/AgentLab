use std::env;

use sqlx::PgPool;

/// 创建全局 PostgreSQL 连接池。
///
/// 通过 `DATABASE_URL` 环境变量连接，供 RAG、Memory 等模块复用。
pub async fn get_db_client() -> PgPool {
    dotenvy::dotenv().ok();
    let database_url = env::var("DATABASE_URL").expect("DATABASE_URL is not empty");
    PgPool::connect(&database_url)
        .await
        .expect("database connection build failed")
}
