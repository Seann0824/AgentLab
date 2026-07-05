use agent_lab::{
    agent::simple_agent::SimpleAgent,
    base::{agent::Agent, config::Config, llm::AgentsLLM},
    tools::{ToolManager, memory::MemoryTool, types::Tool},
};

#[tokio::main]
async fn main() {
    if let Err(e) = run_neo4j_graph_memory_case().await {
        eprintln!("\n❌ Neo4j 图关系 Agent 案例失败: {}", e);
        std::process::exit(1);
    }
    println!("\n✅ Neo4j 图关系 Agent 案例完成");
}

/// 一个专门展示 Neo4j 在 Agent 记忆系统中作用的案例。
///
/// 设计思路：
/// - PG + pgvector 擅长召回“文本语义相似”的记忆。
/// - Neo4j 擅长召回“实体关系相关”的记忆，即使文本本身和查询关键词不相似。
///
/// 本案例会录入 18 条记忆：其中 3 条是回答核心问题所必需的，其余是干扰项。
/// 核心问题为：“小美是通过什么场合认识杭州科技大学的人的？”
///
/// 回答该问题必须组合：
///   记忆 A：小林毕业于杭州科技大学（建立小林 ↔ 杭州科技大学）
///   记忆 B：张伟是杭州科技大学的教授（建立张伟 ↔ 杭州科技大学）
///   记忆 C：上个月公司年会上，小林把张伟介绍给了自己的女朋友小美
///            （该记忆不含“杭州科技大学”字样，向量相似度低，但图关系上把三人串了起来）
///
/// 在默认 limit=5 的召回下，PG 向量检索很容易把记忆 C 挤出前五；
/// Neo4j 通过 `Memory -[:HAS_ENTITY]-> Entity <-[:HAS_ENTITY]- Memory`
/// 以及 `Entity -[:RELATED_TO]-> Entity` 的图扩散，把记忆 C 重新找回来。
async fn run_neo4j_graph_memory_case() -> Result<(), String> {
    // 前置清理：保证每次运行环境干净，避免旧数据干扰演示效果。
    // let cleanup_tool = MemoryTool::new().await;
    // let _ = cleanup_tool
    //     .execute(serde_json::json!({
    //         "action": "clear_all",
    //         "memory_type": "semantic"
    //     }))
    //     .await;

    let mut graph_agent = SimpleAgent::new(
        "图关系记忆助手",
        AgentsLLM::from_env(),
        "你是一个基于长期记忆回答用户问题的助手。\n\
         规则：\n\
         1. 当用户告诉你关于人物、地点、组织、事件的事实时，必须立即调用 memory 工具的 add 动作保存，memory_type 使用 semantic。\n\
         2. 当用户询问涉及多个人或事物之间关系的问题时，必须调用 memory 工具的 search 动作查找，memory_type 使用 semantic；为提高召回率，limit 请设为 10。\n\
         3. 回答时要说明你是根据哪些记忆片段以及它们之间的什么关系得出结论的，不要编造。".to_string(),
        Config::from_env(),
        ToolManager::new()
            .with_tool(Box::new(MemoryTool::new().await)),
        true,
    );

    println!("\n=== 阶段 1：录入 18 条记忆（含核心事实与干扰项）===");

    // 核心事实：3 条记忆组合起来才能回答问题
    let core_facts = vec![
        "小林毕业于杭州科技大学计算机学院",
        "张伟是杭州科技大学人工智能学院的教授",
        "上个月公司年会上，小林把张伟介绍给了自己的女朋友小美",
    ];

    // 干扰项：与主题相关但单独看无法直接回答问题，用来挤占向量检索的 top-K 位置
    let distractors = vec![
        "小美目前在阿里云做大模型产品经理",
        "小林和小美已经恋爱两年了",
        "张伟的研究方向是大模型安全",
        "张伟经常受邀给阿里云做技术分享",
        "小林每天早上八点起床",
        "小林周末喜欢去西湖骑行",
        "杭州科技大学的校园在杭州市余杭区",
        "杭州科技大学的校训是求是创新",
        "阿里云总部位于杭州未来科技城",
        "小美的大学室友上周结婚了",
        "张伟今年发表了 3 篇顶会论文",
        "小林的公司主要做跨境电商",
        "公司年会在杭州萧山的一个酒店举办",
        "小美平时喜欢做瑜伽",
        "杭州亚运会吸引了大量游客",
    ];

    for fact in core_facts.iter().chain(distractors.iter()) {
        graph_agent.run(&format!("请记住：{}", fact)).await;
        println!();
    }

    println!("\n=== 阶段 2：清空对话历史，测试图关系驱动的跨记忆推理 ===");

    graph_agent.clear_history();
    let answer1 = graph_agent
        .run("小美是通过什么场合认识杭州科技大学的人的？请说明推理链条。")
        .await;
    println!("\n[问题1答案]\n{}", answer1);

    graph_agent.clear_history();
    let answer2 = graph_agent
        .run("小林的社交圈里，哪些人和杭州科技大学有关？分别是什么关系？")
        .await;
    println!("\n[问题2答案]\n{}", answer2);

    graph_agent.clear_history();
    let answer3 = graph_agent.run("张伟和小美之间有什么交集？").await;
    println!("\n[问题3答案]\n{}", answer3);

    println!("\n=== 为什么这个案例能体现 Neo4j 的作用 ===");
    println!("1. 核心答案记忆（“公司年会介绍”）不含“杭州科技大学”关键词，向量相似度天然偏低。");
    println!("2. 默认 top-5 召回下，PG 向量检索很容易被 15 条干扰项挤占，漏掉关键记忆。");
    println!("3. Neo4j 把每条记忆抽取成实体节点与关系边，形成跨记忆的知识网络。");
    println!(
        "4. 查询“小美 + 杭州科技大学”时，图检索能沿着 小美-介绍-小林-毕业-杭科大 / 小美-介绍-张伟-任职-杭科大 的多跳路径召回关键记忆。"
    );
    println!("5. 最终 Agent 能把“公司年会介绍”和“杭科大校友/教授”两条记忆组合成完整的因果链条。");

    Ok(())
}
