use serde::{Deserialize, Serialize};

use crate::PromptPolicy;

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
