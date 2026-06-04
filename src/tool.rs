trait Tool {
  fn name(&self) -> &'static str;
  fn description(&self) -> &'static str;
  fn run(&self, input: &str) -> anyhow::Result<String>;
}