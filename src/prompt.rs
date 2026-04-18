use crate::tool::ToolRegistry;
use std::fs;
use std::path::Path;

/// Builder for constructing system prompts with optional sections.
pub struct SystemPromptBuilder {
    sections: Vec<String>,
}

impl SystemPromptBuilder {
    /// Creates a new `SystemPromptBuilder` by loading the default prompt from a specific base path.
    /// If the file `prompts/default.md` does not exist at the given base path, it falls back to an empty builder.
    pub fn new_from_path(base_path: &Path) -> Self {
        let mut builder = Self { sections: Vec::new() };
        let prompt_path = base_path.join("prompts").join("default.md");

        match fs::read_to_string(&prompt_path) {
            Ok(content) => {
                if !content.trim().is_empty() {
                    builder.sections.push(content);
                }
            }
            Err(e) => {
                log::warn!(
                    "Could not load default prompt from {:?}: {}. Using empty prompt.",
                    prompt_path,
                    e
                );
            }
        }
        builder
    }

    /// Creates a new `SystemPromptBuilder` and loads the default prompt from the current directory.
    pub fn new() -> Self {
        let current_dir = std::env::current_dir().unwrap_or_else(|_| std::path::PathBuf::from("."));
        Self::new_from_path(&current_dir)
    }

    /// Creates a builder pre‑configured for a personal assistant role.
    pub fn personal_assistant() -> Self {
        Self::new()
    }

    /// Creates a builder pre‑configured for a personal assistant role from a specific base path.
    pub fn personal_assistant_from_path(base_path: &Path) -> Self {
        Self::new_from_path(base_path)
    }

    /// Adds a role section to the prompt.
    pub fn role(mut self, role: impl Into<String>) -> Self {
        self.sections.push(format!("Role: {}", role.into()));
        self
    }

    /// Adds a guideline section to the prompt.
    pub fn guideline(mut self, text: impl Into<String>) -> Self {
        self.sections.push(format!("Guideline: {}", text.into()));
        self
    }

    /// Adds an environment variable entry to the prompt.
    pub fn environment(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.sections
            .push(format!("Environment {} = {}", key.into(), value.into()));
        self
    }

    /// Adds a custom instruction section to the prompt.
    pub fn custom_instruction(mut self, text: impl Into<String>) -> Self {
        self.sections.push(format!("Instruction: {}", text.into()));
        self
    }

    /// Adds an extra arbitrary section to the prompt.
    pub fn extra_section(mut self, text: impl Into<String>) -> Self {
        self.sections.push(text.into());
        self
    }

    /// Appends descriptions of all registered tools to the prompt.
    pub fn with_tools(mut self, registry: &ToolRegistry) -> Self {
        let mut tool_desc = String::new();
        for tool in &registry.tools {
            let def = tool.definition();
            tool_desc.push_str(&format!(
                "## {}\n\n{}\n\nInput schema:\n```json\n{}\n```\n\n---\n\n",
                def.name,
                def.description,
                serde_json::to_string_pretty(&def.input_schema).unwrap_or_else(|_| "{}".to_string())
            ));
        }
        self.sections.push(tool_desc);
        self
    }

    /// Builds the final prompt string by joining all sections.
    pub fn build(&self) -> String {
        self.sections.join("\n")
    }
}

impl Default for SystemPromptBuilder {
    fn default() -> Self {
        Self::new()
    }
}
