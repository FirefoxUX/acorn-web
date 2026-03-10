//! Pipeline stages that run in sequence: discovery, dependency walking, transformation,
//! code generation, and documentation processing.

pub mod codegen;
pub mod dependency_walker;
pub mod discovery;
pub mod docs;
pub mod fluent;
pub mod svg;
pub mod writer;
