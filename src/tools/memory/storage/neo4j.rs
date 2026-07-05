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
                        MERGE (m)-[:HAS_ENTITY]->(n:Entity {id: $id})
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
