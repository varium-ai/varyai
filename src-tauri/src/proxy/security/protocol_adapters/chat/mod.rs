//! Disabled lane slot; native behavior is gated on CORE-1 and lane acceptance.
pub(in crate::proxy::security) mod history;
pub(in crate::proxy::security) mod request;
pub(in crate::proxy::security) mod response;
#[cfg(test)]
mod tests;
