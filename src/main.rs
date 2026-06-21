use agent_lab::{agent::simple_agent::SimpleAgent, base::agent::Agent};

#[tokio::main]
async fn main() -> () {
    let mut simple_agent = SimpleAgent::new();
    let _ = simple_agent.run("查询比特币价格").await;
}