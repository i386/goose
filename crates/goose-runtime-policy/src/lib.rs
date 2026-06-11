use serde::{Deserialize, Serialize};

pub trait PromptPolicyApplier {
    fn apply_prompt_policy(&mut self, policy: &PromptPolicy);
}

pub trait PromptPolicyTarget {
    fn set_system_prompt_override_from_policy(&mut self, template: String);
    fn add_system_prompt_extra_from_policy(&mut self, key: String, instruction: String);
}

impl<T> PromptPolicyApplier for T
where
    T: PromptPolicyTarget,
{
    fn apply_prompt_policy(&mut self, policy: &PromptPolicy) {
        apply_prompt_policy_to_target(self, policy);
    }
}

pub fn apply_prompt_policy_to_target<T>(target: &mut T, policy: &PromptPolicy)
where
    T: PromptPolicyTarget + ?Sized,
{
    if let Some(template) = &policy.system_prompt_override {
        target.set_system_prompt_override_from_policy(template.clone());
    }

    for extra in &policy.system_prompt_extras {
        target.add_system_prompt_extra_from_policy(extra.key.clone(), extra.instruction.clone());
    }
}

pub fn render_prompt_policy_addendum(policy: &PromptPolicy) -> Option<String> {
    let assembly = PromptAssembly::new("")
        .with_heading("Runtime Policy")
        .with_sections(
            policy
                .system_prompt_extras
                .iter()
                .map(|extra| PromptSection::new(extra.key.clone(), extra.instruction.clone())),
        );

    let rendered = assembly.render();
    rendered
        .strip_prefix("\n\n")
        .map(str::to_string)
        .filter(|addendum| !addendum.trim().is_empty())
}

pub fn render_additional_instructions<'a>(
    base_prompt: impl Into<String>,
    instructions: impl IntoIterator<Item = &'a str>,
) -> String {
    let sections = instructions
        .into_iter()
        .enumerate()
        .map(|(index, instruction)| PromptSection::new(format!("instruction-{index}"), instruction))
        .collect::<Vec<_>>();

    PromptAssembly::new(base_prompt)
        .with_sections(sections)
        .render()
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PromptSection {
    pub key: String,
    pub content: String,
}

impl PromptSection {
    pub fn new(key: impl Into<String>, content: impl Into<String>) -> Self {
        Self {
            key: key.into(),
            content: content.into(),
        }
    }

    pub fn is_empty(&self) -> bool {
        self.content.trim().is_empty()
    }
}

#[derive(Clone, Debug, Eq, PartialEq, Serialize, Deserialize)]
pub struct PromptAssembly {
    pub base_prompt: String,
    pub additional_heading: String,
    pub sections: Vec<PromptSection>,
}

impl PromptAssembly {
    pub fn new(base_prompt: impl Into<String>) -> Self {
        Self {
            base_prompt: base_prompt.into(),
            additional_heading: "Additional Instructions".to_string(),
            sections: Vec::new(),
        }
    }

    pub fn from_policy(base_prompt: impl Into<String>, policy: &PromptPolicy) -> Self {
        Self::new(base_prompt).with_sections(
            policy
                .system_prompt_extras
                .iter()
                .map(|extra| PromptSection::new(extra.key.clone(), extra.instruction.clone())),
        )
    }

    pub fn with_heading(mut self, heading: impl Into<String>) -> Self {
        self.additional_heading = heading.into();
        self
    }

    pub fn with_section(mut self, section: PromptSection) -> Self {
        self.sections.push(section);
        self
    }

    pub fn with_sections(mut self, sections: impl IntoIterator<Item = PromptSection>) -> Self {
        self.sections.extend(sections);
        self
    }

    pub fn render(&self) -> String {
        let section_content = self
            .sections
            .iter()
            .filter(|section| !section.is_empty())
            .map(|section| section.content.as_str())
            .collect::<Vec<_>>();

        if section_content.is_empty() {
            self.base_prompt.clone()
        } else {
            format!(
                "{}\n\n# {}:\n\n{}",
                self.base_prompt,
                self.additional_heading,
                section_content.join("\n\n")
            )
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
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

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Default)]
    struct TestPromptTarget {
        override_template: Option<String>,
        extras: Vec<(String, String)>,
    }

    impl PromptPolicyTarget for TestPromptTarget {
        fn set_system_prompt_override_from_policy(&mut self, template: String) {
            self.override_template = Some(template);
        }

        fn add_system_prompt_extra_from_policy(&mut self, key: String, instruction: String) {
            self.extras.push((key, instruction));
        }
    }

    #[test]
    fn applies_prompt_policy_to_target() {
        let policy = PromptPolicy::chat()
            .with_override("custom base")
            .with_extra("product", "Prefer collaborative wording.")
            .with_extra("tenant", "Respect workspace policy.");
        let mut target = TestPromptTarget::default();

        target.apply_prompt_policy(&policy);

        assert_eq!(target.override_template.as_deref(), Some("custom base"));
        assert_eq!(
            target.extras,
            vec![
                (
                    "product".to_string(),
                    "Prefer collaborative wording.".to_string()
                ),
                (
                    "tenant".to_string(),
                    "Respect workspace policy.".to_string()
                ),
            ]
        );
    }

    #[test]
    fn renders_additional_instructions() {
        let prompt =
            render_additional_instructions("base", ["First instruction", "", "Second instruction"]);

        assert_eq!(
            prompt,
            "base\n\n# Additional Instructions:\n\nFirst instruction\n\nSecond instruction"
        );
    }

    #[test]
    fn skips_section_when_no_additional_instructions() {
        let prompt = render_additional_instructions("base", ["", "   "]);

        assert_eq!(prompt, "base");
    }

    #[test]
    fn prompt_assembly_renders_named_sections() {
        let prompt = PromptAssembly::new("base")
            .with_section(PromptSection::new(
                "product",
                "Prefer collaborative wording.",
            ))
            .with_section(PromptSection::new("tenant", "Respect workspace policy."))
            .render();

        assert_eq!(
            prompt,
            "base\n\n# Additional Instructions:\n\nPrefer collaborative wording.\n\nRespect workspace policy."
        );
    }

    #[test]
    fn prompt_assembly_can_customize_heading_and_skip_empty_sections() {
        let prompt = PromptAssembly::new("base")
            .with_heading("Runtime Policy")
            .with_section(PromptSection::new("empty", " "))
            .with_section(PromptSection::new("policy", "Do the thing."))
            .render();

        assert_eq!(prompt, "base\n\n# Runtime Policy:\n\nDo the thing.");
    }

    #[test]
    fn prompt_policy_layer_renders_named_host_policy() {
        let layer = PromptPolicyLayer::new("host")
            .with_title("Host Policy")
            .with_instruction("Prefer collaborative wording.")
            .with_instruction("")
            .with_instruction("Respect session workspace boundaries.");

        assert_eq!(
            layer.render(),
            "## Host Policy\n\nPrefer collaborative wording.\n\nRespect session workspace boundaries."
        );
    }

    #[test]
    fn prompt_policy_can_be_created_for_explicit_runtime() {
        let policy = PromptPolicy::new(RuntimeMode::Auto, RuntimePlatform::Desktop)
            .with_layer(PromptPolicyLayer::new("host").with_instruction("Use host permissions."));

        assert_eq!(policy.runtime_mode, RuntimeMode::Auto);
        assert_eq!(policy.runtime_platform, RuntimePlatform::Desktop);
        assert_eq!(policy.system_prompt_extras[0].key, "host");
        assert_eq!(
            policy.system_prompt_extras[0].instruction,
            "Use host permissions."
        );
    }

    #[test]
    fn prompt_assembly_can_render_from_policy_without_base_copying() {
        let policy = PromptPolicy::chat().with_layer(
            PromptPolicyLayer::new("host")
                .with_title("Host Policy")
                .with_instruction("Use host permissions."),
        );

        let prompt = PromptAssembly::from_policy("goose base", &policy).render();

        assert_eq!(
            prompt,
            "goose base\n\n# Additional Instructions:\n\n## Host Policy\n\nUse host permissions."
        );
    }

    #[test]
    fn renders_policy_addendum_for_hosts() {
        let policy = PromptPolicy::chat()
            .with_extra("host", "Use host permissions.")
            .with_extra("tenant", "");

        let addendum = render_prompt_policy_addendum(&policy).unwrap();

        assert_eq!(addendum, "# Runtime Policy:\n\nUse host permissions.");
    }
}
