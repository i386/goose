use crate::*;

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
