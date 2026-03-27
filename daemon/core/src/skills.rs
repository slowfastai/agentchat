use std::path::{Component, Path, PathBuf};

use agentchat_protocol::SkillInfo;

pub(crate) const SHARED_SKILL_PREFIX: &str = "shared/";
pub(crate) const AGENT_SKILL_ROOT: &str = "agents";

/// Stores reusable project knowledge as markdown files.
pub struct SkillStore {
    skills_dir: PathBuf,
}

impl SkillStore {
    pub fn new(project_root: &Path) -> Self {
        Self {
            skills_dir: project_root.join(".agentchat").join("skills"),
        }
    }

    pub async fn list_skills(&self) -> Result<Vec<SkillInfo>, String> {
        if !self.skills_dir.exists() {
            return Ok(Vec::new());
        }

        let mut pending_dirs = vec![self.skills_dir.clone()];
        let mut skills = Vec::new();

        while let Some(dir) = pending_dirs.pop() {
            let mut entries = tokio::fs::read_dir(&dir)
                .await
                .map_err(|e| format!("failed to read skills dir {}: {e}", dir.display()))?;

            while let Some(entry) = entries
                .next_entry()
                .await
                .map_err(|e| format!("failed to read skills dir entry: {e}"))?
            {
                let path = entry.path();
                let file_type = entry
                    .file_type()
                    .await
                    .map_err(|e| format!("failed to stat skill {}: {e}", path.display()))?;

                if file_type.is_dir() {
                    pending_dirs.push(path);
                    continue;
                }

                if !file_type.is_file()
                    || path.extension().and_then(|ext| ext.to_str()) != Some("md")
                {
                    continue;
                }

                let metadata = entry
                    .metadata()
                    .await
                    .map_err(|e| format!("failed to stat skill {}: {e}", path.display()))?;
                let Some(relative) = path.strip_prefix(&self.skills_dir).ok() else {
                    continue;
                };
                let Some(name) = relative_skill_name(relative) else {
                    continue;
                };

                skills.push(SkillInfo {
                    path: format!(".agentchat/skills/{name}"),
                    name,
                    size_bytes: metadata.len(),
                });
            }
        }

        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(skills)
    }

    pub async fn list_shared_skills(&self) -> Result<Vec<SkillInfo>, String> {
        self.list_skills_matching(is_shared_skill_name).await
    }

    pub async fn list_agent_skills(&self, agent_id: &str) -> Result<Vec<SkillInfo>, String> {
        let prefix = agent_skill_prefix(agent_id)?;
        self.list_skills_matching(|name| name.starts_with(&prefix))
            .await
    }

    pub async fn read_skill(&self, name: &str) -> Result<String, String> {
        let path = self.resolve_skill_path(name)?;

        tokio::fs::read_to_string(&path)
            .await
            .map_err(|e| format!("failed to read skill {}: {e}", path.display()))
    }

    pub async fn write_skill(&self, name: &str, content: &str) -> Result<(), String> {
        let path = self.resolve_skill_path(name)?;

        if let Some(parent) = path.parent() {
            tokio::fs::create_dir_all(parent)
                .await
                .map_err(|e| format!("failed to create skills dir: {e}"))?;
        }

        tokio::fs::write(&path, content)
            .await
            .map_err(|e| format!("failed to write skill {}: {e}", path.display()))
    }

    pub fn skills_dir(&self) -> &Path {
        &self.skills_dir
    }

    fn resolve_skill_path(&self, name: &str) -> Result<PathBuf, String> {
        let normalized = normalize_skill_name(name)?;
        Ok(self.skills_dir.join(normalized))
    }

    async fn list_skills_matching<F>(&self, mut predicate: F) -> Result<Vec<SkillInfo>, String>
    where
        F: FnMut(&str) -> bool,
    {
        let mut skills = self.list_skills().await?;
        skills.retain(|skill| predicate(&skill.name));
        Ok(skills)
    }
}

pub(crate) fn is_shared_skill_name(name: &str) -> bool {
    !name.contains('/') || name.starts_with(SHARED_SKILL_PREFIX)
}

pub(crate) fn agent_skill_prefix(agent_id: &str) -> Result<String, String> {
    Ok(format!(
        "{AGENT_SKILL_ROOT}/{}/",
        normalize_path_segment(agent_id, "agent id")?
    ))
}

fn relative_skill_name(path: &Path) -> Option<String> {
    let parts = path
        .components()
        .map(|component| match component {
            Component::Normal(part) => part.to_str().map(|part| part.to_string()),
            _ => None,
        })
        .collect::<Option<Vec<_>>>()?;

    if parts.is_empty() {
        None
    } else {
        Some(parts.join("/"))
    }
}

fn normalize_skill_name(name: &str) -> Result<String, String> {
    normalize_relative_name(name, "skill name")
}

fn normalize_path_segment(segment: &str, kind: &str) -> Result<String, String> {
    let trimmed = segment.trim();
    if trimmed.is_empty() {
        return Err(format!("{kind} cannot be empty"));
    }

    let path = Path::new(trimmed);
    if path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return Err(format!("invalid {kind}: {trimmed}"));
    }

    let part = path
        .file_name()
        .and_then(|part| part.to_str())
        .ok_or_else(|| format!("invalid {kind}: {trimmed}"))?;
    if part.contains("..") {
        return Err(format!("invalid {kind}: {trimmed}"));
    }

    Ok(part.to_string())
}

fn normalize_relative_name(name: &str, kind: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err(format!("{kind} cannot be empty"));
    }

    let path = Path::new(trimmed);
    if path.is_absolute() {
        return Err(format!("invalid {kind}: {trimmed}"));
    }

    let mut parts = Vec::new();
    for component in path.components() {
        let Component::Normal(part) = component else {
            return Err(format!("invalid {kind}: {trimmed}"));
        };

        let part = part
            .to_str()
            .ok_or_else(|| format!("invalid {kind}: {trimmed}"))?;
        if part.is_empty() || part.contains("..") {
            return Err(format!("invalid {kind}: {trimmed}"));
        }

        parts.push(part.to_string());
    }

    let Some(file_name) = parts.last_mut() else {
        return Err(format!("invalid {kind}: {trimmed}"));
    };

    if !file_name.ends_with(".md") {
        *file_name = format!("{file_name}.md");
    }

    Ok(parts.join("/"))
}

#[cfg(test)]
mod tests {
    use tempfile::tempdir;

    use super::*;

    #[tokio::test(flavor = "current_thread")]
    async fn list_skills_returns_empty_when_no_skills() {
        let root = tempdir().unwrap();
        let store = SkillStore::new(root.path());

        assert!(store.list_skills().await.unwrap().is_empty());
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_then_read_skill_round_trips() {
        let root = tempdir().unwrap();
        let store = SkillStore::new(root.path());

        store.write_skill("testing", "# Testing\n").await.unwrap();

        assert_eq!(store.read_skill("testing").await.unwrap(), "# Testing\n");
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_then_read_shared_skill_round_trips() {
        let root = tempdir().unwrap();
        let store = SkillStore::new(root.path());

        store
            .write_skill("shared/testing", "# Shared Testing\n")
            .await
            .unwrap();

        assert_eq!(
            store.read_skill("shared/testing").await.unwrap(),
            "# Shared Testing\n"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_then_read_agent_specific_skill_round_trips() {
        let root = tempdir().unwrap();
        let store = SkillStore::new(root.path());

        store
            .write_skill("agents/fake/testing", "# Fake Testing\n")
            .await
            .unwrap();

        assert_eq!(
            store.read_skill("agents/fake/testing").await.unwrap(),
            "# Fake Testing\n"
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_skills_returns_written_skills_in_nested_directories() {
        let root = tempdir().unwrap();
        let store = SkillStore::new(root.path());
        store.write_skill("beta", "beta").await.unwrap();
        store.write_skill("shared/alpha", "alpha").await.unwrap();
        store
            .write_skill("agents/fake/private", "private")
            .await
            .unwrap();

        let skills = store.list_skills().await.unwrap();
        let names = skills
            .into_iter()
            .map(|skill| skill.name)
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec!["agents/fake/private.md", "beta.md", "shared/alpha.md"]
        );
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_shared_skills_includes_legacy_top_level_skills() {
        let root = tempdir().unwrap();
        let store = SkillStore::new(root.path());
        store.write_skill("legacy", "legacy").await.unwrap();
        store.write_skill("shared/common", "common").await.unwrap();
        store
            .write_skill("agents/fake/private", "private")
            .await
            .unwrap();

        let names = store
            .list_shared_skills()
            .await
            .unwrap()
            .into_iter()
            .map(|skill| skill.name)
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["legacy.md", "shared/common.md"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn list_agent_skills_returns_only_matching_agent_namespace() {
        let root = tempdir().unwrap();
        let store = SkillStore::new(root.path());
        store
            .write_skill("agents/fake/private", "private")
            .await
            .unwrap();
        store
            .write_skill("agents/other/private", "other")
            .await
            .unwrap();
        store.write_skill("shared/common", "common").await.unwrap();

        let names = store
            .list_agent_skills("fake")
            .await
            .unwrap()
            .into_iter()
            .map(|skill| skill.name)
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["agents/fake/private.md"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_skill_rejects_path_traversal() {
        let root = tempdir().unwrap();
        let store = SkillStore::new(root.path());

        let error = store.read_skill("../secret.md").await.unwrap_err();
        assert!(error.contains("invalid skill name"));
    }

    #[tokio::test(flavor = "current_thread")]
    async fn write_skill_rejects_nested_path_traversal() {
        let root = tempdir().unwrap();
        let store = SkillStore::new(root.path());

        let error = store
            .write_skill("shared/../secret", "nope")
            .await
            .unwrap_err();
        assert!(error.contains("invalid skill name"));
    }

    #[test]
    fn agent_skill_prefix_rejects_invalid_agent_ids() {
        let error = agent_skill_prefix("../fake").unwrap_err();

        assert!(error.contains("invalid agent id"));
    }
}
