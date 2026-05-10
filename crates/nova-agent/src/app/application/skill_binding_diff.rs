pub(super) fn should_emit_skill_bindings_updated(
    before: &[nova_protocol::observability::SkillBindingSnapshot],
    after: &[nova_protocol::observability::SkillBindingSnapshot],
) -> bool {
    let before_fingerprint = skill_binding_fingerprint(before);
    let after_fingerprint = skill_binding_fingerprint(after);
    before_fingerprint != after_fingerprint
}

fn skill_binding_fingerprint(
    skills: &[nova_protocol::observability::SkillBindingSnapshot],
) -> Vec<(String, String, String, Option<String>)> {
    let mut entries = skills
        .iter()
        .map(|skill| {
            (
                skill.skill_id.clone(),
                skill.name.clone(),
                skill.status.clone(),
                skill.description.clone(),
            )
        })
        .collect::<Vec<_>>();
    entries.sort_unstable_by(|left, right| left.0.cmp(&right.0));
    entries
}
