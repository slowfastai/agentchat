use std::path::{Component, Path, PathBuf};

use agentchat_protocol::SkillInfo;

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

        let mut entries = tokio::fs::read_dir(&self.skills_dir)
            .await
            .map_err(|e| format!("failed to read skills dir: {e}"))?;
        let mut skills = Vec::new();

        while let Some(entry) = entries
            .next_entry()
            .await
            .map_err(|e| format!("failed to read skills dir entry: {e}"))?
        {
            let path = entry.path();
            if path.extension().and_then(|ext| ext.to_str()) != Some("md") {
                continue;
            }

            let metadata = entry
                .metadata()
                .await
                .map_err(|e| format!("failed to stat skill {}: {e}", path.display()))?;
            let Some(name) = path.file_name().and_then(|name| name.to_str()) else {
                continue;
            };

            skills.push(SkillInfo {
                name: name.to_string(),
                path: format!(".agentchat/skills/{name}"),
                size_bytes: metadata.len(),
            });
        }

        skills.sort_by(|a, b| a.name.cmp(&b.name));
        Ok(skills)
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
}

fn normalize_skill_name(name: &str) -> Result<String, String> {
    let trimmed = name.trim();
    if trimmed.is_empty() {
        return Err("skill name cannot be empty".into());
    }

    let path = Path::new(trimmed);
    if path.components().count() != 1
        || !matches!(path.components().next(), Some(Component::Normal(_)))
    {
        return Err(format!("invalid skill name: {trimmed}"));
    }

    let file_name = path
        .file_name()
        .and_then(|file_name| file_name.to_str())
        .ok_or_else(|| format!("invalid skill name: {trimmed}"))?;

    if file_name.contains("..") {
        return Err(format!("invalid skill name: {trimmed}"));
    }

    if file_name.ends_with(".md") {
        Ok(file_name.to_string())
    } else {
        Ok(format!("{file_name}.md"))
    }
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
    async fn list_skills_returns_written_skills() {
        let root = tempdir().unwrap();
        let store = SkillStore::new(root.path());
        store.write_skill("beta", "beta").await.unwrap();
        store.write_skill("alpha", "alpha").await.unwrap();

        let skills = store.list_skills().await.unwrap();
        let names = skills
            .into_iter()
            .map(|skill| skill.name)
            .collect::<Vec<_>>();

        assert_eq!(names, vec!["alpha.md", "beta.md"]);
    }

    #[tokio::test(flavor = "current_thread")]
    async fn read_skill_rejects_path_traversal() {
        let root = tempdir().unwrap();
        let store = SkillStore::new(root.path());

        let error = store.read_skill("../secret.md").await.unwrap_err();
        assert!(error.contains("invalid skill name"));
    }
}
