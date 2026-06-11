use crate::ToolInvocationArgumentError;

pub type ToolInvocationArguments = serde_json::Map<String, serde_json::Value>;

pub fn take_invocation_argument_object(
    arguments: Option<serde_json::Value>,
) -> Result<Option<ToolInvocationArguments>, ToolInvocationArgumentError> {
    match arguments {
        Some(serde_json::Value::Object(arguments)) => Ok(Some(arguments)),
        Some(_) => Err(ToolInvocationArgumentError::new(
            "Tool arguments must be a JSON object.",
        )),
        None => Ok(None),
    }
}
