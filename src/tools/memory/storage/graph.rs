/// 根据实体 name + type 计算稳定的 entity id。
///
/// 使用 64-bit FNV-1a 算法，保证跨进程、跨运行得到的 hash 一致，
/// 从而让相同 name+type 的实体在 Neo4j 中对应同一个 `:Entity` 节点。
pub fn entity_id(name: &str, entity_type: &str) -> String {
    const FNV_OFFSET: u64 = 0xcbf29ce484222325;
    const FNV_PRIME: u64 = 0x100000001b3;

    // 对 name/type 做规范化，避免大小写、前后空格等导致同一实体产生多个节点。
    let name = name.trim().to_lowercase();
    let entity_type = entity_type.trim().to_lowercase();
    let key = format!("{}:{}", name, entity_type);
    let mut hash = FNV_OFFSET;
    for byte in key.bytes() {
        hash ^= byte as u64;
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    format!("{:016x}", hash)
}
