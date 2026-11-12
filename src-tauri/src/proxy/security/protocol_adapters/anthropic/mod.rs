//! Disabled native lane; AN1 exposes request inspection without route or history authority.
pub(in crate::proxy::security) mod history;
mod request;
pub(in crate::proxy::security) mod response;
#[cfg(test)]
mod tests;

#[cfg(test)]
pub(in crate::proxy::security) use request::observe_request;
