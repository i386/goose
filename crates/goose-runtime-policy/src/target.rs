use crate::PromptPolicy;

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
