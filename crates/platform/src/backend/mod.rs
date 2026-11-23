//! Backend implementations of the platform contract.
//!
//! Exactly one backend is compiled in. Everything a backend owns stays behind
//! this module: no type declared under `backend` appears in the crate's public
//! surface.

mod winit_backend;
mod winit_input;

pub(crate) use winit_backend::run;
