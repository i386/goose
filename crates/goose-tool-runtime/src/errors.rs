use std::fmt;

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolRuntimePolicyError {
    Unavailable { tool_name: String },
    NotAllowed { tool_name: String },
    RequiresApproval { tool_name: String },
    Denied { tool_name: String },
}

impl fmt::Display for ToolRuntimePolicyError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolRuntimePolicyError::Unavailable { tool_name } => {
                write!(f, "Tool `{tool_name}` is not available in this session.")
            }
            ToolRuntimePolicyError::NotAllowed { tool_name } => {
                write!(f, "Tool `{tool_name}` is not allowed in this session.")
            }
            ToolRuntimePolicyError::RequiresApproval { tool_name } => write!(
                f,
                "Tool `{tool_name}` requires host approval before dispatch."
            ),
            ToolRuntimePolicyError::Denied { tool_name } => {
                write!(
                    f,
                    "Tool `{tool_name}` was denied by the session tool policy."
                )
            }
        }
    }
}

impl std::error::Error for ToolRuntimePolicyError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum ToolDispatchPreparationError {
    Policy(ToolRuntimePolicyError),
    Arguments(ToolInvocationArgumentError),
}

impl fmt::Display for ToolDispatchPreparationError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ToolDispatchPreparationError::Policy(error) => write!(f, "{error}"),
            ToolDispatchPreparationError::Arguments(error) => write!(f, "{error}"),
        }
    }
}

impl std::error::Error for ToolDispatchPreparationError {}

impl From<ToolRuntimePolicyError> for ToolDispatchPreparationError {
    fn from(error: ToolRuntimePolicyError) -> Self {
        Self::Policy(error)
    }
}

impl From<ToolInvocationArgumentError> for ToolDispatchPreparationError {
    fn from(error: ToolInvocationArgumentError) -> Self {
        Self::Arguments(error)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolInvocationArgumentError {
    pub message: String,
}

impl ToolInvocationArgumentError {
    pub fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }
}

impl fmt::Display for ToolInvocationArgumentError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.message)
    }
}

impl std::error::Error for ToolInvocationArgumentError {}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ToolRegistryPolicyViolation {
    pub missing_allowed: Vec<String>,
    pub disallowed_active: Vec<String>,
}

impl ToolRegistryPolicyViolation {
    pub fn is_empty(&self) -> bool {
        self.missing_allowed.is_empty() && self.disallowed_active.is_empty()
    }
}

impl fmt::Display for ToolRegistryPolicyViolation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if !self.missing_allowed.is_empty() {
            write!(
                f,
                "tool policy cannot allow unavailable tools: {}",
                self.missing_allowed.join(", ")
            )?;
        }

        if !self.disallowed_active.is_empty() {
            if !self.missing_allowed.is_empty() {
                write!(f, "; ")?;
            }
            write!(
                f,
                "tool policy requires selective filtering, but active disallowed tools are still registered: {}",
                self.disallowed_active.join(", ")
            )?;
        }

        Ok(())
    }
}

impl std::error::Error for ToolRegistryPolicyViolation {}
