// The engine, second implementation: the sans-IO core on the virtual host
// (conformance), the real host with `run` (M1), the http client and the desk
// (M2), the MCP surface (M3).
pub mod check;
pub mod confine;
pub mod desk;
pub mod engine;
pub mod fixture;
pub mod host;
pub mod http;
pub mod json;
pub mod mcp;
pub mod net;
pub mod peers;
pub mod proxy;
pub mod run;
pub mod runner;
pub mod trace;
