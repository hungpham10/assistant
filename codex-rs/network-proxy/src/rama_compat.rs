//! Small compatibility shims for API that rama 0.4 no longer provides.

use rama_core::error::BoxError;
use rama_core::extensions::Extension;
use rama_net::address::HostWithPort;

/// Authority of the target a proxied connection should be forwarded to.
#[derive(Debug, Clone)]
pub(crate) struct ProxyTarget(pub HostWithPort);

impl Extension for ProxyTarget {}

/// Build a boxed error from a plain message.
pub(crate) fn boxed_error(message: impl Into<String>) -> BoxError {
    BoxError::from(message.into())
}
