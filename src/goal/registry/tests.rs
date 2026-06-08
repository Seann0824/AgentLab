use super::*;
use tempfile::TempDir;

#[test]
fn test_registry_create_and_get() {
    let tmp_dir = TempDir::new().unwrap();
    let root = tmp_dir.path().to_string_lossy().to_string();

    let mut registry = GoalRegistry::new(&root);
    let goal = Goal::new(
        "测试目标".to_string(),
        "描述".to_string(),
        vec!["条件1".to_string()],
    );
    let id = goal.id.clone();

    registry.create(goal).unwrap();
    let loaded = registry.get(&id).unwrap();
    assert_eq!(loaded.name, "测试目标");
    assert_eq!(loaded.status, GoalStatus::Proposed);
}

#[test]
fn test_registry_update() {
    let tmp_dir = TempDir::new().unwrap();
    let root = tmp_dir.path().to_string_lossy().to_string();

    let mut registry = GoalRegistry::new(&root);
    let goal = Goal::new("测试".to_string(), "".to_string(), vec![]);
    let id = goal.id.clone();
    registry.create(goal).unwrap();

    let mut loaded = registry.get(&id).unwrap().clone();
    loaded.activate();
    registry.update(loaded).unwrap();

    let updated = registry.get(&id).unwrap();
    assert_eq!(updated.status, GoalStatus::Active);
}

#[test]
fn test_registry_load_all() {
    let tmp_dir = TempDir::new().unwrap();
    let root = tmp_dir.path().to_string_lossy().to_string();

    let mut registry = GoalRegistry::new(&root);
    let mut g1 = Goal::new("目标1".to_string(), "".to_string(), vec![]);
    let mut g2 = Goal::new("目标2".to_string(), "".to_string(), vec![]);
    g1.id = "test_load_all_1".to_string();
    g2.id = "test_load_all_2".to_string();
    let id1 = g1.id.clone();
    let id2 = g2.id.clone();

    registry.create(g1).unwrap();
    registry.create(g2).unwrap();

    // 新建一个 registry 并加载
    let mut registry2 = GoalRegistry::new(&root);
    registry2.load_all().unwrap();

    assert_eq!(registry2.list().len(), 2);
    assert!(registry2.get(&id1).is_some());
    assert!(registry2.get(&id2).is_some());
}

#[test]
fn test_active_goal() {
    let tmp_dir = TempDir::new().unwrap();
    let root = tmp_dir.path().to_string_lossy().to_string();

    let mut registry = GoalRegistry::new(&root);
    let mut goal = Goal::new("活跃目标".to_string(), "".to_string(), vec![]);
    let id = goal.id.clone();
    goal.activate();
    registry.create(goal).unwrap();

    assert!(registry.has_active_goal());
    let active = registry.active_goal().unwrap();
    assert_eq!(active.id, id);
}

#[test]
fn test_delete() {
    let tmp_dir = TempDir::new().unwrap();
    let root = tmp_dir.path().to_string_lossy().to_string();

    let mut registry = GoalRegistry::new(&root);
    let goal = Goal::new("待删除".to_string(), "".to_string(), vec![]);
    let id = goal.id.clone();
    registry.create(goal).unwrap();
    assert_eq!(registry.list().len(), 1);

    registry.delete(&id).unwrap();
    assert_eq!(registry.list().len(), 0);
}

#[test]
fn test_goal_context_prompt() {
    let tmp_dir = TempDir::new().unwrap();
    let root = tmp_dir.path().to_string_lossy().to_string();

    let mut registry = GoalRegistry::new(&root);
    let mut goal = Goal::new(
        "测试目标".to_string(),
        "测试描述".to_string(),
        vec!["条件A".to_string(), "条件B".to_string()],
    );
    goal.steps = vec!["步骤1".to_string(), "步骤2".to_string()];
    goal.activate();
    goal.record_step_completed("步骤1".to_string());
    registry.create(goal).unwrap();

    let prompt = registry.get_goal_context_prompt().unwrap();
    assert!(prompt.contains("测试目标"));
    assert!(prompt.contains("条件A"));
    assert!(prompt.contains("步骤1"));
    assert!(prompt.contains("🎯"));
}
