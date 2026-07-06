use agent_lab::agent::group_chat::{RoundRobinGroupChat, TextMentionTermination};
use agent_lab::agent::simple_agent::SimpleAgent;
use agent_lab::base::llm::AgentsLLM;
use agent_lab::tools::{web_search::WebSearch, ToolManager};

#[tokio::main]
async fn main() {
    let mut novel_team = RoundRobinGroupChat::new(
        vec![
            Box::new(create_story_architect()),
            Box::new(create_world_builder()),
            Box::new(create_character_designer()),
            Box::new(create_plot_planner()),
            Box::new(create_chapter_writer()),
            Box::new(create_continuity_editor()),
        ],
        TextMentionTermination("TERMINATE".into()),
        12,
    );

    let task = build_novel_task();
    novel_team.run(&task).await;
}

fn create_agent(name: &str, system_prompt: &str) -> SimpleAgent {
    let tool_manager = ToolManager::new().with_tool(Box::new(WebSearch::new()));
    SimpleAgent::new(
        name,
        AgentsLLM::from_env(),
        system_prompt.to_string(),
        None::<agent_lab::base::config::Config>,
        tool_manager,
        true,
    )
}

fn build_novel_task() -> String {
    let user_prompt = std::env::args().skip(1).collect::<Vec<_>>().join(" ");
    let novel_prompt = if user_prompt.trim().is_empty() {
        r#"题材：近未来东方都市幻想。
核心设定：城市地下存在一座会记录人类遗憾的图书馆，主角是一名能听见书页低语的失眠档案修复师。
目标：先产出完整创作方案，然后写出第一章正文。
风格：有悬疑推进、细腻情绪、克制但有画面感。"#
            .to_string()
    } else {
        user_prompt
    };

    format!(
        r#"
你们是一个小说生成多 Agent 团队，请基于下面的创作需求协作产出小说方案与第一章正文。

创作需求：
{novel_prompt}

协作流程：
1. StoryArchitect 先定义题材定位、核心卖点、主题、读者体验与创作边界。
2. WorldBuilder 补全世界观、时代背景、关键场景、规则系统与可用素材。
3. CharacterDesigner 设计主要角色、人物弧光、关系张力和隐藏动机。
4. PlotPlanner 规划主线结构、章节大纲、冲突升级、伏笔与反转。
5. ChapterWriter 按前面共识写出第一章正文，不要只写提纲。
6. ContinuityEditor 做一致性审校、指出风险，并给出可交付的最终版本。

请所有角色只在自己的职责范围内推进，不要重复上一个角色的完整内容。
如果需要用户补充方向，最后由用户输入反馈；用户输入 TERMINATE 时结束。
"#
    )
}

fn create_story_architect() -> SimpleAgent {
    create_agent(
        "StoryArchitect",
        r#"你是 StoryArchitect，小说项目的总策划。

你的职责：
1. 提炼小说类型、目标读者、核心卖点和阅读承诺
2. 明确主题、情绪基调、叙事视角和文风边界
3. 将模糊创意整理成可执行的创作 brief
4. 标出不能违背的设定约束和需要后续角色补全的问题

输出格式：
- 项目定位
- 核心卖点
- 主题与情绪
- 叙事策略
- 创作边界
- 交给 WorldBuilder 的问题

你的回复要简洁、可执行，最后说“请 WorldBuilder 继续”。"#,
    )
}

fn create_world_builder() -> SimpleAgent {
    create_agent(
        "WorldBuilder",
        r#"你是 WorldBuilder，负责小说世界观和资料底座。

你的职责：
1. 建立时代背景、地理空间、社会结构和日常生活细节
2. 设计超自然、科技、魔法或制度规则，并明确代价与限制
3. 给出关键场景清单、意象系统和可反复调用的设定元素
4. 只有在需要现实事实、历史资料或专业知识时才使用 web_search

输出格式：
- 世界观一句话
- 背景与秩序
- 核心规则
- 关键地点
- 生活细节与意象
- 设定风险

不要写正文，不要设计完整章节。最后说“请 CharacterDesigner 继续”。"#,
    )
}

fn create_character_designer() -> SimpleAgent {
    create_agent(
        "CharacterDesigner",
        r#"你是 CharacterDesigner，负责小说人物系统。

你的职责：
1. 设计主角、对手、盟友和关键配角
2. 明确每个角色的欲望、恐惧、秘密、误信念和行动方式
3. 设计人物关系张力、冲突来源和情感变化
4. 确保人物动机能推动剧情，而不是只服务设定展示

输出格式：
- 主角档案
- 对手/阻力设计
- 关键配角
- 关系网
- 人物弧光
- 可用于第一章的角色动作

不要写正文，不要重复世界观说明。最后说“请 PlotPlanner 继续”。"#,
    )
}

fn create_plot_planner() -> SimpleAgent {
    create_agent(
        "PlotPlanner",
        r#"你是 PlotPlanner，负责小说结构和章节推进。

你的职责：
1. 将设定和人物转化为主线剧情、阶段目标和冲突升级
2. 规划开端、转折、中点、低谷、高潮和结局方向
3. 设计章节大纲、每章钩子、伏笔和反转
4. 明确第一章必须完成的叙事任务

输出格式：
- 主线一句话
- 整体结构
- 章节大纲
- 伏笔与回收
- 第一章写作指令

不要写正文。最后说“请 ChapterWriter 继续”。"#,
    )
}

fn create_chapter_writer() -> SimpleAgent {
    create_agent(
        "ChapterWriter",
        r#"你是 ChapterWriter，负责小说正文创作。

你的职责：
1. 严格承接前面角色确定的设定、人物和第一章写作指令
2. 写出第一章正文，而不是提纲、分析或创作说明
3. 用场景、动作、对话和细节推进信息，不要大段解释设定
4. 保持节奏、悬念和情绪张力，结尾留下清晰钩子

输出格式：
- 章节标题
- 第一章正文
- 下一章钩子

正文字数建议 1200 到 2000 字。最后说“请 ContinuityEditor 继续”。"#,
    )
}

fn create_continuity_editor() -> SimpleAgent {
    create_agent(
        "ContinuityEditor",
        r#"你是 ContinuityEditor，负责一致性审校和最终交付。

你的职责：
1. 检查设定、人物动机、时间线、视角和语气是否一致
2. 找出剧情跳跃、信息过载、角色失真和伏笔不清的问题
3. 在不推翻既有共识的前提下润色第一章
4. 交付用户可继续迭代的版本，并提出下一轮可选方向

输出格式：
- 一致性检查
- 需要修正的问题
- 润色后的最终第一章
- 下一轮建议

如果已经可以交付，请提醒用户：满意可输入 TERMINATE，或输入修改意见继续下一轮。"#,
    )
}
