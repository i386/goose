use serde::{Deserialize, Serialize};

pub struct PromptPolicy {
    pub system_prompt_extras: Vec<SystemPromptExtra>,
    pub system_prompt_override: Option<String>,
    pub runtime_mode: RuntimeMode,
    pub runtime_platform: RuntimePlatform,
}

impl PromptPolicy {
    pub fn new(runtime_mode: RuntimeMode, runtime_platform: RuntimePlatform) -> Self {
        Self {
            system_prompt_extras: Vec::new(),
            system_prompt_override: None,
            runtime_mode,
            runtime_platform,
        }
    }

    pub fn chat() -> Self {
        Self::new(RuntimeMode::Chat, RuntimePlatform::Cli)
    }

    pub fn with_extra(mut self, key: impl Into<String>, instruction: impl Into<String>) -> Self {
        self.system_prompt_extras.push(SystemPromptExtra {
            key: key.into(),
            instruction: instruction.into(),
        });
        self
    }

    pub fn with_layer(self, layer: PromptPolicyLayer) -> Self {
        let instruction = layer.render();
        self.with_extra(layer.key, instruction)
    }

    pub fn with_override(mut self, template: impl Into<String>) -> Self {
        self.system_prompt_override = Some(template.into());
        self
    }

    pub fn merge(mut self, other: PromptPolicy) -> Self {
        if other.system_prompt_override.is_some() {
            self.system_prompt_override = other.system_prompt_override;
        }
        self.system_prompt_extras.extend(other.system_prompt_extras);
        self.runtime_mode = other.runtime_mode;
        self.runtime_platform = other.runtime_platform;
        self
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SystemPromptExtra {
    pub key: String,
    pub instruction: String,
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PromptPolicyLayer {
    pub key: String,
    pub title: Option<String>,
    pub instructions: Vec<String>,
}

impl PromptPolicyLayer {
    pub fn new(key: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            title: None,
            instructions: Vec::new(),
        }
    }

    pub fn with_title(mut self, title: impl Into<String>) -> Self {
        self.title = Some(title.into());
        self
    }

    pub fn with_instruction(mut self, instruction: impl Into<String>) -> Self {
        self.instructions.push(instruction.into());
        self
    }

    pub fn render(&self) -> String {
        let instructions = self
            .instructions
            .iter()
            .map(|instruction| instruction.trim())
            .filter(|instruction| !instruction.is_empty())
            .collect::<Vec<_>>();

        match (&self.title, instructions.as_slice()) {
            (Some(title), []) => format!("## {title}"),
            (Some(title), instructions) => format!("## {title}\n\n{}", instructions.join("\n\n")),
            (None, instructions) => instructions.join("\n\n"),
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimeMode {
    Auto,
    Approve,
    SmartApprove,
    Chat,
}

#[derive(Clone, Copy, Debug, Eq, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RuntimePlatform {
    Cli,
    Desktop,
}
