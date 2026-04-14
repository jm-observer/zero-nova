use crate::tool::ToolRegistry;

pub struct SystemPromptBuilder {
    sections: Vec<String>,
}

impl SystemPromptBuilder {
    pub fn new() -> Self {
        Self { sections: Vec::new() }
    }

    pub fn personal_assistant() -> Self {
        let mut builder = Self::new();
        builder
            .sections
            .push("You are a helpful personal assistant.".to_string());
        builder
    }

    pub fn role(mut self, role: impl Into<String>) -> Self {
        self.sections.push(format!("Role: {}", role.into()));
        self
    }

    pub fn guideline(mut self, text: impl Into<String>) -> Self {
        self.sections.push(format!("Guideline: {}", text.into()));
        self
    }

    pub fn environment(mut self, key: impl Into<String>, value: impl Into<String>) -> Self {
        self.sections
            .push(format!("Environment {} = {}", key.into(), value.into()));
        self
    }

    pub fn custom_instruction(mut self, text: impl Into<String>) -> Self {
        self.sections.push(format!("Instruction: {}", text.into()));
        self
    }

    pub fn extra_section(mut self, text: impl Into<String>) -> Self {
        self.sections.push(text.into());
        self
    }

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

    pub fn build(&self) -> String {
        self.sections.join("\n")
    }
}

impl Default for SystemPromptBuilder {
    fn default() -> Self {
        Self::new()
    }
}
