/// Neo4j 中全局共享的实体引用节点。
///
/// `id` 由业务层根据 name + type 计算出的稳定 hash 生成，不依赖 LLM。
/// 全文内容保存在 PG 中，通过 `(:Memory)-[:HAS_ENTITY]->(:Entity)` 进行双向绑定。
#[derive(Clone, Debug)]
pub struct Entity {
    pub id: String,
    pub name: String,
    pub entity_type: String,
}

/// 实体之间的关系引用，隶属于某一条具体的记忆。
#[derive(Clone, Debug)]
pub struct Relation {
    pub from_id: String,
    pub to_id: String,
    pub relation_type: String,
    pub memory_id: String,
    pub user_id: String,
}

/// 子 agent / Tool 调用传入的原始实体，不含 id，由存储层补全。
#[derive(Clone, Debug, serde::Deserialize)]
pub struct EntityInput {
    pub name: String,
    #[serde(rename = "type")]
    pub entity_type: String,
}

/// 子 agent / Tool 调用传入的原始关系，不含 id，由存储层补全。
#[derive(Clone, Debug, serde::Deserialize)]
pub struct RelationInput {
    pub from_name: String,
    #[serde(rename = "from_type")]
    pub from_entity_type: String,
    pub relation_type: String,
    pub to_name: String,
    #[serde(rename = "to_type")]
    pub to_entity_type: String,
}

#[derive(Clone)]
pub struct Neo4jStore {
    graph: neo4rs::Graph,
}

impl Neo4jStore {
    pub async fn new(
        uri: impl Into<String>,
        user: impl Into<String>,
        password: impl Into<String>,
    ) -> Result<Self, String> {
        let graph = neo4rs::Graph::new(uri, user, password)
            .await
            .map_err(|e| format!("[Neo4jStore] connection failed: {}", e))?;

        // 为 Entity.id 和 Memory 的复合键建立唯一约束，确保 MERGE 的原子性，
        // 避免并发/重复调用时产生重复节点。
        // 如果数据库中已存在重复数据，创建约束会失败；这里记录 warning 而不是阻断启动，
        // 由运维侧在清理重复数据后重新触发约束创建。
        if let Err(e) = graph
            .run(neo4rs::query(
                "CREATE CONSTRAINT entity_id_unique IF NOT EXISTS FOR (e:Entity) REQUIRE e.id IS UNIQUE",
            ))
            .await
        {
            tracing::warn!(
                "[Neo4jStore] create Entity constraint skipped (probably duplicate entities exist): {}",
                e
            );
        }

        if let Err(e) = graph
            .run(neo4rs::query(
                "CREATE CONSTRAINT memory_id_user_unique IF NOT EXISTS FOR (m:Memory) REQUIRE (m.memory_id, m.user_id) IS UNIQUE",
            ))
            .await
        {
            tracing::warn!(
                "[Neo4jStore] create Memory constraint skipped (probably duplicate memories exist): {}",
                e
            );
        }

        Ok(Self { graph })
    }

    /// 写入一条记忆对应的引用图。
    ///
    /// 会创建/更新轻量的 `:Memory` 引用节点，并通过 `:HAS_ENTITY` 关联到全局 `:Entity` 节点，
    /// 实体之间的关系以 `:RELATED_TO` 边存储。
    pub async fn add_reference_graph(
        &self,
        memory_id: impl Into<String>,
        user_id: impl Into<String>,
        memory_type: impl Into<String>,
        entities: &[Entity],
        relations: &[Relation],
    ) -> Result<(), String> {
        let memory_id = memory_id.into();
        let user_id = user_id.into();
        let memory_type = memory_type.into();

        self.graph
            .run(
                neo4rs::query(
                    r#"
                    MERGE (m:Memory {memory_id: $memory_id, user_id: $user_id})
                    SET m.memory_type = $memory_type
                    "#,
                )
                .param("memory_id", memory_id.clone())
                .param("user_id", user_id.clone())
                .param("memory_type", memory_type),
            )
            .await
            .map_err(|e| format!("[Neo4jStore] create memory node failed: {}", e))?;

        for e in entities {
            self.graph
                .run(
                    neo4rs::query(
                        r#"
                        MERGE (n:Entity {id: $id})
                        SET n.name = $name,
                            n.type = $type
                        "#,
                    )
                    .param("id", e.id.clone())
                    .param("name", e.name.clone())
                    .param("type", e.entity_type.clone()),
                )
                .await
                .map_err(|e| format!("[Neo4jStore] merge entity failed: {}", e))?;

            self.graph
                .run(
                    neo4rs::query(
                        r#"
                        MATCH (m:Memory {memory_id: $memory_id, user_id: $user_id})
                        MATCH (n:Entity {id: $id})
                        MERGE (m)-[:HAS_ENTITY]->(n)
                        "#,
                    )
                    .param("memory_id", memory_id.clone())
                    .param("user_id", user_id.clone())
                    .param("id", e.id.clone()),
                )
                .await
                .map_err(|e| format!("[Neo4jStore] link entity to memory failed: {}", e))?;
        }

        for r in relations {
            self.graph
                .run(
                    neo4rs::query(
                        r#"
                        MATCH (a:Entity {id: $from_id})
                        MATCH (b:Entity {id: $to_id})
                        MERGE (a)-[rel:RELATED_TO]->(b)
                        SET rel.relation_type = $relation_type,
                            rel.memory_id = $memory_id,
                            rel.user_id = $user_id
                        "#,
                    )
                    .param("from_id", r.from_id.clone())
                    .param("to_id", r.to_id.clone())
                    .param("relation_type", r.relation_type.clone())
                    .param("memory_id", memory_id.clone())
                    .param("user_id", user_id.clone()),
                )
                .await
                .map_err(|e| format!("[Neo4jStore] add relation failed: {}", e))?;
        }

        Ok(())
    }

    /// 根据一个 `memory_id` 在实体关系图中查找关联的其他 `memory_id`。
    ///
    /// 路径：Memory -> HAS_ENTITY -> Entity -> RELATED_TO -> Entity <- HAS_ENTITY <- Memory
    /// 只返回同一 `user_id` 下的记忆引用，保证数据按用户隔离。
    pub async fn get_related_memory_ids(
        &self,
        memory_id: impl Into<String>,
        user_id: impl Into<String>,
        depth: i64,
        limit: usize,
    ) -> Result<Vec<String>, String> {
        let memory_id = memory_id.into();
        let user_id = user_id.into();

        let depth = depth.max(1);
        let cypher = format!(
            r#"
            MATCH (m:Memory {{memory_id: $memory_id, user_id: $user_id}})
                  -[:HAS_ENTITY]->(e:Entity)
                  -[:RELATED_TO*1..{}]->(related:Entity)
                  <-[:HAS_ENTITY]-(related_m:Memory)
            WHERE related_m.user_id = $user_id
              AND related_m.memory_id <> $memory_id
            RETURN DISTINCT related_m.memory_id AS memory_id
            LIMIT $limit
            "#,
            depth
        );

        let mut stream = self
            .graph
            .execute(
                neo4rs::query(&cypher)
                    .param("memory_id", memory_id)
                    .param("user_id", user_id)
                    .param("limit", limit as i64),
            )
            .await
            .map_err(|e| format!("[Neo4jStore] get related memory ids failed: {}", e))?;

        let mut results = Vec::new();
        while let Ok(Some(row)) = stream.next().await {
            let id: String = row
                .get("memory_id")
                .map_err(|e| format!("[Neo4jStore] parse related memory_id failed: {}", e))?;
            results.push(id);
        }

        Ok(results)
    }

    /// 根据一组实体 id 查找关联的记忆 id。
    ///
    /// 返回 `(memory_id, matched_count)`，按命中实体数降序排列，
    /// `matched_count` 可用于计算图相关性分数。
    pub async fn get_memory_ids_by_entities(
        &self,
        user_id: impl Into<String>,
        entity_ids: &[String],
        limit: usize,
    ) -> Result<Vec<(String, i64)>, String> {
        let user_id = user_id.into();

        let mut stream = self
            .graph
            .execute(
                neo4rs::query(
                    r#"
                    MATCH (e:Entity)<-[:HAS_ENTITY]-(m:Memory)
                    WHERE e.id IN $entity_ids AND m.user_id = $user_id
                    RETURN m.memory_id AS memory_id, count(DISTINCT e.id) AS matched_count
                    ORDER BY matched_count DESC
                    LIMIT $limit
                    "#,
                )
                .param("entity_ids", entity_ids)
                .param("user_id", user_id)
                .param("limit", limit as i64),
            )
            .await
            .map_err(|e| format!("[Neo4jStore] get memory ids by entities failed: {}", e))?;

        let mut results = Vec::new();
        while let Ok(Some(row)) = stream.next().await {
            let memory_id: String = row
                .get("memory_id")
                .map_err(|e| format!("[Neo4jStore] parse memory_id failed: {}", e))?;
            let matched_count: i64 = row
                .get("matched_count")
                .map_err(|e| format!("[Neo4jStore] parse matched_count failed: {}", e))?;
            results.push((memory_id, matched_count));
        }

        Ok(results)
    }

    /// 根据一组实体 id，通过实体之间的关系图查找关联记忆。
    ///
    /// 路径：查询实体 -> `RELATED_TO*1..depth` -> 相关实体 <- `HAS_ENTITY` - 记忆
    /// 返回 `(memory_id, related_entity_count)`，按命中相关实体数降序排列。
    pub async fn get_related_memory_ids_by_entities(
        &self,
        user_id: impl Into<String>,
        entity_ids: &[String],
        depth: i64,
        limit: usize,
    ) -> Result<Vec<(String, i64)>, String> {
        let user_id = user_id.into();
        let depth = depth.max(1);

        let cypher = format!(
            r#"
            MATCH (qe:Entity)
            WHERE qe.id IN $entity_ids
            MATCH (qe)-[:RELATED_TO*1..{}]-(re:Entity)<-[:HAS_ENTITY]-(m:Memory)
            WHERE m.user_id = $user_id
            RETURN m.memory_id AS memory_id, count(DISTINCT re.id) AS related_count
            ORDER BY related_count DESC
            LIMIT $limit
            "#,
            depth
        );

        let mut stream = self
            .graph
            .execute(
                neo4rs::query(&cypher)
                    .param("entity_ids", entity_ids)
                    .param("user_id", user_id)
                    .param("limit", limit as i64),
            )
            .await
            .map_err(|e| format!("[Neo4jStore] get related memory ids by entities failed: {}", e))?;

        let mut results = Vec::new();
        while let Ok(Some(row)) = stream.next().await {
            let memory_id: String = row
                .get("memory_id")
                .map_err(|e| format!("[Neo4jStore] parse memory_id failed: {}", e))?;
            let related_count: i64 = row
                .get("related_count")
                .map_err(|e| format!("[Neo4jStore] parse related_count failed: {}", e))?;
            results.push((memory_id, related_count));
        }

        Ok(results)
    }

    /// 删除某条记忆在 Neo4j 中的引用图（按 memory_id + user_id）。
    pub async fn delete_reference_graph(
        &self,
        memory_id: impl Into<String>,
        user_id: impl Into<String>,
    ) -> Result<(), String> {
        self.graph
            .run(
                neo4rs::query(
                    r#"
                    MATCH (m:Memory {memory_id: $memory_id, user_id: $user_id})
                    OPTIONAL MATCH (m)-[he:HAS_ENTITY]->(:Entity)
                    OPTIONAL MATCH ()-[r:RELATED_TO {memory_id: $memory_id}]->()
                    DELETE he, r
                    DETACH DELETE m
                    "#,
                )
                .param("memory_id", memory_id.into())
                .param("user_id", user_id.into()),
            )
            .await
            .map_err(|e| format!("[Neo4jStore] delete reference graph failed: {}", e))?;

        Ok(())
    }

    /// 删除某一类型（memory_type）的所有记忆引用图。
    pub async fn delete_reference_graph_by_memory_type(
        &self,
        memory_type: impl Into<String>,
    ) -> Result<(), String> {
        let memory_type = memory_type.into();

        // 1) 先删除该类型下所有 memory_id 关联的 RELATED_TO 关系。
        //    HAS_ENTITY 关系会在 DETACH DELETE m 时一并删除。
        self.graph
            .run(
                neo4rs::query(
                    r#"
                    MATCH (m:Memory {memory_type: $memory_type})
                    MATCH ()-[r:RELATED_TO]->()
                    WHERE r.memory_id = m.memory_id
                    DELETE r
                    "#,
                )
                .param("memory_type", memory_type.clone()),
            )
            .await
            .map_err(|e| format!("[Neo4jStore] delete RELATED_TO by memory_type failed: {}", e))?;

        // 2) 删除该类型的所有 Memory 节点（连带 HAS_ENTITY 关系）。
        self.graph
            .run(
                neo4rs::query(
                    r#"
                    MATCH (m:Memory {memory_type: $memory_type})
                    DETACH DELETE m
                    "#,
                )
                .param("memory_type", memory_type),
            )
            .await
            .map_err(|e| format!("[Neo4jStore] delete Memory by memory_type failed: {}", e))?;

        Ok(())
    }

    /// 删除某条记忆在 Neo4j 中的引用图（仅按 memory_id）。
    pub async fn delete_reference_graph_by_memory(
        &self,
        memory_id: impl Into<String>,
    ) -> Result<(), String> {
        self.graph
            .run(
                neo4rs::query(
                    r#"
                    MATCH (m:Memory {memory_id: $memory_id})
                    OPTIONAL MATCH (m)-[he:HAS_ENTITY]->(:Entity)
                    OPTIONAL MATCH ()-[r:RELATED_TO {memory_id: $memory_id}]->()
                    DELETE he, r
                    DETACH DELETE m
                    "#,
                )
                .param("memory_id", memory_id.into()),
            )
            .await
            .map_err(|e| format!("[Neo4jStore] delete reference graph by memory failed: {}", e))?;

        Ok(())
    }
}
