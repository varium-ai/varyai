//! Direct native Gemini adapters; authority still comes only from handler admission.
pub(in crate::proxy::security) mod history;
pub(in crate::proxy::security) mod request;
pub(in crate::proxy::security) mod response;
#[cfg(test)]
mod tests;
