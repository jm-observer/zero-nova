use crate::tool::ToolRegistry;
use std::fs;
use std::path::Path;

/// Builder for constructing system prompts with optional sections.
pub struct SystemPromptBuilder {
    sections: Vec<String>,
}

impl SystemPromptBuilder {

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
        Self {
            sections: vec![],
        }
    }
}
