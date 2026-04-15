use crate::tool::ToolRegistry;

/// Builder for constructing system prompts with optional sections.
pub struct SystemPromptBuilder {
    sections: Vec<String>,
}

impl SystemPromptBuilder {
    /// Creates a new SystemPromptBuilder and loads the default prompt.
    pub fn new() -> Self {
        // Load default prompt from the prompts/default.md file at compile time.
        // If the file contains any content (non‑empty after trimming), it will be added as the initial section.
        let mut builder = Self { sections: Vec::new() };
        let default_content = include_str!("../prompts/default.md");
        if !default_content.trim().is_empty() {
            builder.sections.push(default_content.to_string());
        }
        builder
    }

    /// Creates a builder pre‑configured for a personal assistant role.
    pub fn personal_assistant() -> Self {
        let mut builder = Self::new();
        builder
            .sections
            .push("You are a helpful personal assistant.".to_string());
        builder
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
