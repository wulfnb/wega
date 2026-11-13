//! Security policy primitives extracted from `wega-engine`: Content Security
//! Policy and the pure trustworthy-origin check. V8 secure-context exposure
//! stays in the engine; CORS stays in `wega-network`.
#![forbid(unsafe_code)]

pub mod csp;
pub mod secure_context;
