pub(in crate::ui) fn stack_name_from_labels(labels: &str) -> Option<String> {
    // docker ps --format exposes labels as a comma-separated "k=v" list.
    // Compose stacks typically set:
    // - com.docker.compose.project=<stack>
    // Swarm stacks often set:
    // - com.docker.stack.namespace=<stack>
    for part in labels.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let Some((k, v)) = part.split_once('=') else {
            continue;
        };
        let k = k.trim();
        let v = v.trim();
        if v.is_empty() {
            continue;
        }
        if k == "com.docker.compose.project" || k == "com.docker.stack.namespace" {
            return Some(v.to_string());
        }
    }
    None
}
