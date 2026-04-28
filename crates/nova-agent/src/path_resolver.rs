use std::path::{Component, Path, PathBuf};

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PathRefOrigin {
    RelativeToProject,
    Absolute,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ResolvedPathRef {
    pub target_path: PathBuf,
    pub is_dir: bool,
    pub origin: PathRefOrigin,
}

#[derive(Debug, PartialEq, Eq)]
pub enum PathResolveError {
    InvalidPathSyntax {
        raw_input: String,
        action: String,
    },
    PathNotFound {
        raw_input: String,
        resolved_path: String,
        action: String,
    },
    PathAccessDenied {
        raw_input: String,
        resolved_path: String,
        action: String,
    },
}

impl std::fmt::Display for PathResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::InvalidPathSyntax { raw_input, action } => {
                write!(f, "InvalidPathSyntax: '{}'. {}", raw_input, action)
            }
            Self::PathNotFound {
                raw_input,
                resolved_path,
                action,
            } => write!(f, "PathNotFound: '{}' -> '{}'. {}", raw_input, resolved_path, action),
            Self::PathAccessDenied {
                raw_input,
                resolved_path,
                action,
            } => write!(
                f,
                "PathAccessDenied: '{}' -> '{}'. {}",
                raw_input, resolved_path, action
            ),
        }
    }
}

pub fn resolve_path_ref(
    raw_ref: &str,
    project_dir: &Path,
    allowed_root: Option<&Path>,
    require_exists: bool,
) -> Result<ResolvedPathRef, PathResolveError> {
    let trimmed = raw_ref.trim();
    if trimmed.is_empty() {
        return Err(PathResolveError::InvalidPathSyntax {
            raw_input: raw_ref.to_string(),
            action: "Provide a non-empty path after '@'.".to_string(),
        });
    }

    let with_at = if let Some(stripped) = trimmed.strip_prefix('@') {
        stripped.trim()
    } else {
        trimmed
    };
    if with_at.is_empty() {
        return Err(PathResolveError::InvalidPathSyntax {
            raw_input: raw_ref.to_string(),
            action: "Use '@<path>', for example '@src/lib.rs'.".to_string(),
        });
    }

    let raw_path = Path::new(with_at);
    let (resolved_path, origin) = if raw_path.is_absolute() {
        (raw_path.to_path_buf(), PathRefOrigin::Absolute)
    } else {
        (project_dir.join(raw_path), PathRefOrigin::RelativeToProject)
    };

    if matches!(origin, PathRefOrigin::RelativeToProject) && !is_within_root(&resolved_path, project_dir) {
        return Err(PathResolveError::PathAccessDenied {
            raw_input: raw_ref.to_string(),
            resolved_path: resolved_path.display().to_string(),
            action: "Use a path inside the current project directory.".to_string(),
        });
    }

    if let Some(root) = allowed_root {
        if !is_within_root(&resolved_path, root) {
            return Err(PathResolveError::PathAccessDenied {
                raw_input: raw_ref.to_string(),
                resolved_path: resolved_path.display().to_string(),
                action: "Use a path inside the allowed root directory.".to_string(),
            });
        }
    }

    if require_exists && !resolved_path.exists() {
        return Err(PathResolveError::PathNotFound {
            raw_input: raw_ref.to_string(),
            resolved_path: resolved_path.display().to_string(),
            action: "Check the path and create the file or directory if needed.".to_string(),
        });
    }

    let is_dir = resolved_path.is_dir();
    Ok(ResolvedPathRef {
        target_path: resolved_path,
        is_dir,
        origin,
    })
}

fn is_within_root(path: &Path, root: &Path) -> bool {
    let normalized_path = normalize_path(path);
    let normalized_root = normalize_path(root);
    normalized_path.starts_with(&normalized_root)
}

fn normalize_path(path: &Path) -> PathBuf {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            Component::ParentDir => {
                normalized.pop();
            }
            Component::CurDir => {}
            _ => normalized.push(component),
        }
    }
    normalized
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn temp_dir(prefix: &str) -> PathBuf {
        let suffix = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_nanos();
        let dir = std::env::temp_dir().join(format!("zero-nova-{}-{}", prefix, suffix));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn resolve_relative_path_in_project_dir() {
        let root = temp_dir("path-resolver-relative");
        let file = root.join("src").join("lib.rs");
        fs::create_dir_all(file.parent().unwrap()).unwrap();
        fs::write(&file, "mod a;").unwrap();

        let resolved = resolve_path_ref("@src/lib.rs", &root, None, true).unwrap();
        assert_eq!(resolved.target_path, file);
        assert!(!resolved.is_dir);
        assert_eq!(resolved.origin, PathRefOrigin::RelativeToProject);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn resolve_absolute_path() {
        let root = temp_dir("path-resolver-abs");
        let file = root.join("a.txt");
        fs::write(&file, "x").unwrap();

        let resolved = resolve_path_ref(&format!("@{}", file.display()), &root, None, true).unwrap();
        assert_eq!(resolved.target_path, file);
        assert_eq!(resolved.origin, PathRefOrigin::Absolute);

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn reject_path_escape() {
        let root = temp_dir("path-resolver-escape");
        let file = root.join("..").join("outside.txt");
        let err = resolve_path_ref(&format!("@{}", file.display()), &root, Some(&root), true).unwrap_err();
        assert!(matches!(err, PathResolveError::PathAccessDenied { .. }));

        fs::remove_dir_all(root).unwrap();
    }

    #[test]
    fn write_target_can_be_missing_when_allowed() {
        let root = temp_dir("path-resolver-missing-ok");
        let resolved = resolve_path_ref("@new-dir/new-file.txt", &root, Some(&root), false).unwrap();
        assert_eq!(resolved.origin, PathRefOrigin::RelativeToProject);
        assert!(!resolved.is_dir);

        fs::remove_dir_all(root).unwrap();
    }
}
