-- 启用 pgvector 扩展
CREATE EXTENSION IF NOT EXISTS vector;

-- 创建 memories 表
CREATE TABLE IF NOT EXISTS memories (
    memory_id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL,
    memory_type TEXT NOT NULL,
    content TEXT NOT NULL,
    embedding VECTOR(768) NOT NULL,
    importance FLOAT NOT NULL DEFAULT 0.5,
    timestamp BIGINT NOT NULL,
    session_id TEXT,
    properties JSONB,
    created_at TIMESTAMP DEFAULT NOW()
);

-- 常用过滤索引
CREATE INDEX IF NOT EXISTS idx_memories_type_user
    ON memories(memory_type, user_id);

CREATE INDEX IF NOT EXISTS idx_memories_session
    ON memories(session_id);

CREATE INDEX IF NOT EXISTS idx_memories_timestamp
    ON memories(timestamp);

-- HNSW 向量索引
CREATE INDEX IF NOT EXISTS idx_memories_embedding
    ON memories
    USING hnsw (embedding vector_cosine_ops);

-- 创建 rag_chunks 表（RAG 专用全局资料库，与用户体系解耦）
CREATE TABLE IF NOT EXISTS rag_chunks (
    id TEXT PRIMARY KEY,
    namespace TEXT NOT NULL DEFAULT 'default',
    source TEXT NOT NULL,
    content TEXT NOT NULL,
    embedding VECTOR(768) NOT NULL,
    heading_path TEXT,
    start_pos BIGINT,
    end_pos BIGINT,
    chunk_index INTEGER,
    metadata JSONB,
    created_at TIMESTAMP DEFAULT NOW()
);

-- 常用过滤索引
CREATE INDEX IF NOT EXISTS idx_rag_chunks_namespace
    ON rag_chunks(namespace);

CREATE INDEX IF NOT EXISTS idx_rag_chunks_source
    ON rag_chunks(source);

-- HNSW 向量索引
CREATE INDEX IF NOT EXISTS idx_rag_chunks_embedding
    ON rag_chunks
    USING hnsw (embedding vector_cosine_ops);

-- 会话表
CREATE TABLE IF NOT EXISTS chat_sessions (
    id TEXT PRIMARY KEY,
    user_id TEXT NOT NULL DEFAULT 'default_user',
    title TEXT,
    created_at BIGINT NOT NULL,
    updated_at BIGINT NOT NULL,
    metadata JSONB
);

CREATE INDEX IF NOT EXISTS idx_chat_sessions_user_updated
    ON chat_sessions(user_id, updated_at DESC);

-- 消息表
CREATE TABLE IF NOT EXISTS chat_messages (
    id TEXT PRIMARY KEY,
    session_id TEXT NOT NULL REFERENCES chat_sessions(id) ON DELETE CASCADE,
    role TEXT NOT NULL,
    content TEXT NOT NULL,
    timestamp BIGINT NOT NULL,
    tool_call_id TEXT,
    tool_calls JSONB,
    metadata JSONB,
    seq INTEGER NOT NULL,
    created_at TIMESTAMP DEFAULT NOW()
);

CREATE INDEX IF NOT EXISTS idx_chat_messages_session_seq
    ON chat_messages(session_id, seq);
