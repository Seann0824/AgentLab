use std::env;
use dotenvy;
use crate::agent::{Agent, AssistantAgent, UserProxyAgent};
use crate::model::openai::OpenaiChatCompletionClient;
use crate::tools::web_search::WebSearch;
use crate::tools::ToolManager;
use model::openai;
mod model;
mod tools;
mod agent;
mod autogen_agentchat;
mod core;

#[tokio::main]
async fn main() -> () {
    dotenvy::dotenv().ok();
    let product_manager = create_produt_manager();
    let engineer = create_engineer();
    let code_reviewer = create_code_reviewer();
    let user_proxy = create_user_proxy();

    let mut team_chat = autogen_agentchat::RoundRobinGroupChat::new(
        vec![
            product_manager,
            engineer,
            code_reviewer,
            user_proxy,
        ],
        autogen_agentchat::TextMentionTermination("TERMINATE".into()),
        20
    );

    let task = r#"我们需要开发一个比特币价格显示应用，具体要求如下：
            核心功能：
            - 实时显示比特币当前价格（USD）
            - 显示24小时价格变化趋势（涨跌幅和涨跌额）
            - 提供价格刷新功能

            技术要求：
            - 使用 Streamlit 框架创建 Web 应用
            - 界面简洁美观，用户友好
            - 添加适当的错误处理和加载状态

            请团队协作完成这个任务，从需求分析到最终实现。"#;

   let result = team_chat.run_stream(task).await;
}

fn get_openai_client() -> (OpenaiChatCompletionClient, ToolManager) {
    let api_key = env::var("DEEPSEEK_API_KEY").unwrap();
    let base_url = env::var("DEEPSEEK_BASE_URL").unwrap();
    let model = env::var("DEEPSEEK_MODEL").unwrap();
    let tool_manager = ToolManager::new()
        .register_tool(Box::new(WebSearch::new()));
    let model_client = openai::OpenaiChatCompletionClient::new(model, &api_key, &base_url, None);
    (model_client, tool_manager)
}

fn create_produt_manager() -> agent::Agent {
    let (model_client, tool_manager) = get_openai_client();
    let system_message = r#"
        你是一位经验丰富的产品经理，专门负责软件产品的需求分析和项目规划。

        你的核心职责包括：
        1. **需求分析**：深入理解用户需求，识别核心功能和边界条件
        2. **技术规划**：基于需求制定清晰的技术实现路径
        3. **风险评估**：识别潜在的技术风险和用户体验问题
        4. **协调沟通**：与工程师和其他团队成员进行有效沟通

        当接到开发任务时，请按以下结构进行分析：
        1. 需求理解与分析
        2. 功能模块划分
        3. 技术选型建议
        4. 实现优先级排序
        5. 验收标准定义

        请简洁明了地回应，并在分析完成后说"请工程师开始实现"。
    "#;

    return agent::Agent::AssistantAgent(
        AssistantAgent::new(
            "ProductManager".into(),
            model_client,
            system_message.into(),
            tool_manager
        )
    )
}

fn create_engineer() -> agent::Agent {
    let (model_client, tool_manager) = get_openai_client();
    let system_message = r#"
        你是一位资深的软件工程师，擅长 Python 开发和 Web 应用构建。

        你的技术专长包括：
        1. **Python 编程**：熟练掌握 Python 语法和最佳实践
        2. **Web 开发**：精通 Streamlit、Flask、Django 等框架
        3. **API 集成**：有丰富的第三方 API 集成经验
        4. **错误处理**：注重代码的健壮性和异常处理

        当收到开发任务时，请：
        1. 仔细分析技术需求
        2. 选择合适的技术方案
        3. 编写完整的代码实现
        4. 添加必要的注释和说明
        5. 考虑边界情况和异常处理

        请提供完整的可运行代码，并在完成后说"请代码审查员检查"。
    "#;

    return  agent::Agent::AssistantAgent(
        AssistantAgent::new(
            "Engineer".into(),
            model_client,
            system_message.into(),
            tool_manager
        )
    )
}

fn create_code_reviewer() -> agent::Agent {
    let (model_client, tool_manager) = get_openai_client();
    let system_message = r#"
        你是一位经验丰富的代码审查专家，专注于代码质量和最佳实践。
        你的审查重点包括：
        1. **代码质量**：检查代码的可读性、可维护性和性能
        2. **安全性**：识别潜在的安全漏洞和风险点
        3. **最佳实践**：确保代码遵循行业标准和最佳实践
        4. **错误处理**：验证异常处理的完整性和合理性

        审查流程：
        1. 仔细阅读和理解代码逻辑
        2. 检查代码规范和最佳实践
        3. 识别潜在问题和改进点
        4. 提供具体的修改建议
        5. 评估代码的整体质量

        请提供具体的审查意见，完成后说"代码审查完成，请用户代理测试
    "#;

    return agent::Agent::AssistantAgent(
        AssistantAgent::new(
            "CodeReviewer".into(),
            model_client,
            system_message.into(),
            tool_manager
        )
    )
}

fn create_user_proxy() -> agent::Agent {
    agent::Agent::UserProxyAgent(
        UserProxyAgent::new(
            "UserProxy".into(),
            r#"
                用户代理，负责以下职责：
                1. 代表用户提出开发需求
                2. 执行最终的代码实现
                3. 验证功能是否符合预期
                4. 提供用户反馈和建议

                完成测试后请回复 TERMINATE
            "#.into()
        )
    )
}