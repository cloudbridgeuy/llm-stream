//! ChatGPT subscription authentication.
//!
//! `pkce`, `oauth`, and `token` are a pure functional core: total functions over
//! values, no I/O, no clock. `store`, `listener`, and `flow` are the imperative
//! shell that binds them to sockets, files, and the network.

// pub mod flow;
// pub mod listener;
pub mod oauth;
pub mod pkce;
// pub mod store;
// pub mod token;
